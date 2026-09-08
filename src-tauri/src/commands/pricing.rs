//! Pricing delivery to the ingestion writer; no second writable connection.
use super::runtime::Work;
use crate::{
    pricing::PriceInput,
    storage::{
        self,
        pricing::{DetectedModel, PriceVersion, PRICING_BATCH_SIZE},
        Store,
    },
};
use serde::Serialize;
use std::sync::mpsc;

pub(crate) enum Message {
    Source(notify::Result<notify::Event>),
    Pricing(Request),
    Settings(super::settings::Request),
}

pub(crate) enum Request {
    Catalog {
        after: Option<String>,
        reply: mpsc::Sender<Result<Page, Error>>,
    },
    Save {
        model: String,
        configuration: PriceInput,
        backfill_before: bool,
        reply: mpsc::Sender<Result<PriceVersion, Error>>,
    },
}

#[derive(Clone)]
pub struct Control(pub(crate) mpsc::SyncSender<Message>);

pub(crate) fn channel() -> (Control, mpsc::Receiver<Message>) {
    let (send, receive) = mpsc::sync_channel(256);
    (Control(send), receive)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub models: Vec<DetectedModel>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    pub code: &'static str,
    pub field: Option<&'static str>,
    pub message: String,
}

impl Error {
    fn unavailable() -> Self {
        Self {
            code: "unavailable",
            field: None,
            message: "Pricing is unavailable. Restart the monitor to reconnect.".into(),
        }
    }
}

impl From<storage::Error> for Error {
    fn from(error: storage::Error) -> Self {
        if let storage::Error::Pricing(error) = error {
            use crate::pricing::Error::*;
            let (code, field) = match &error {
                InvalidRate { field } => ("invalid_rate", Some(*field)),
                ReasoningRate => ("reasoning_rate", Some("reasoning")),
                UnknownModel => ("unknown_model", None),
                BackfillOnlyFirst => ("backfill_only_first", Some("backfillBefore")),
                Clock => ("clock", None),
                _ => ("invalid_configuration", None),
            };
            Self {
                code,
                field,
                message: error.to_string(),
            }
        } else {
            Self {
                code: "storage",
                field: None,
                message: "The pricing operation failed. Reload models before retrying a save."
                    .into(),
            }
        }
    }
}

impl Control {
    pub(crate) fn request<T>(
        &self,
        make: impl FnOnce(mpsc::Sender<Result<T, Error>>) -> Request,
    ) -> Result<T, Error> {
        let (reply, receive) = mpsc::channel();
        self.0
            .try_send(Message::Pricing(make(reply)))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => Error {
                    code: "busy",
                    field: None,
                    message: "The monitor is busy. Try again shortly.".into(),
                },
                mpsc::TrySendError::Disconnected(_) => Error::unavailable(),
            })?;
        receive.recv().map_err(|_| Error::unavailable())?
    }
}

pub(crate) fn handle(request: Request, store: &mut Store, work: &mut Work) {
    match request {
        Request::Catalog { after, reply } => {
            let result = store
                .pricing_models(after.as_deref())
                .map(|models| Page {
                    next_cursor: (models.len() == PRICING_BATCH_SIZE)
                        .then(|| models.last().unwrap().model.clone()),
                    models,
                })
                .map_err(Error::from);
            let _ = reply.send(result);
        }
        Request::Save {
            model,
            configuration,
            backfill_before,
            reply,
        } => {
            let result = store
                .save_model_price(&model, configuration, backfill_before)
                .map(|version| {
                    // The version and durable job committed before waking the pricing lane.
                    work.request_pricing();
                    version
                })
                .map_err(Error::from);
            let _ = reply.send(result);
        }
    }
}

#[tauri::command]
pub async fn pricing_models(
    control: tauri::State<'_, Control>,
    after: Option<String>,
) -> Result<Page, Error> {
    let control = control.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        control.request(|reply| Request::Catalog { after, reply })
    })
    .await
    .map_err(|_| Error::unavailable())?
}

#[tauri::command]
pub async fn save_model_price(
    control: tauri::State<'_, Control>,
    model: String,
    configuration: PriceInput,
    backfill_before: bool,
) -> Result<PriceVersion, Error> {
    let control = control.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        control.request(|reply| Request::Save {
            model,
            configuration,
            backfill_before,
            reply,
        })
    })
    .await
    .map_err(|_| Error::unavailable())?
}
