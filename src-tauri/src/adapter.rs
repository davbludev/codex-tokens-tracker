//! Allowlisted projections only. Unknown payload fields are never retained.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

pub const VERSION: &str = "modern-1";

/// Canonical UTC ordering, independent of timezone spelling or fractional precision.
pub fn observation_time(timestamp: &str) -> Result<(i64, u32), &'static str> {
    let value =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
            .map_err(|_| "Invalid usage timestamp")?;
    Ok((value.unix_timestamp(), value.nanosecond()))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Counter {
    #[default]
    Missing,
    Null,
    Known(i64),
}
impl Counter {
    fn missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
    pub fn value(&self) -> Option<i64> {
        if let Self::Known(n) = self {
            Some(*n)
        } else {
            None
        }
    }
}
impl<'de> Deserialize<'de> for Counter {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<i64>::deserialize(d)? {
            Some(n) => Self::Known(n),
            None => Self::Null,
        })
    }
}
impl Serialize for Counter {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Known(n) => s.serialize_i64(*n),
            _ => s.serialize_none(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Tokens {
    #[serde(default, skip_serializing_if = "Counter::missing")]
    pub input_tokens: Counter,
    #[serde(default, skip_serializing_if = "Counter::missing")]
    pub cached_input_tokens: Counter,
    #[serde(default, skip_serializing_if = "Counter::missing")]
    pub cache_write_input_tokens: Counter,
    #[serde(default, skip_serializing_if = "Counter::missing")]
    pub output_tokens: Counter,
    #[serde(default, skip_serializing_if = "Counter::missing")]
    pub reasoning_output_tokens: Counter,
    #[serde(default, skip_serializing_if = "Counter::missing")]
    pub total_tokens: Counter,
}
impl Tokens {
    pub fn values(&self) -> Option<[i64; 6]> {
        Some([
            self.input_tokens.value()?,
            self.cached_input_tokens.value()?,
            self.cache_write_input_tokens.value()?,
            self.output_tokens.value()?,
            self.reasoning_output_tokens.value()?,
            self.total_tokens.value()?,
        ])
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Usage {
    pub thread_id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub root_turn_id: Option<String>,
    pub response_id: Option<String>,
    pub usage: Tokens,
    pub thread_token_usage: Tokens,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Metadata {
    pub id: String,
    pub session_id: Option<String>,
    pub parent_thread_id: Option<String>,
    #[serde(default, skip_deserializing)]
    pub nested_parent_thread_id: Option<String>,
    pub cwd: Option<String>,
    pub workspace_roots: Option<Vec<String>>,
    pub cli_version: Option<String>,
    #[serde(skip_serializing)]
    source: Option<MetadataSource>,
}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MetadataSource {
    Subagent { subagent: Subagent },
    Other(serde::de::IgnoredAny),
}
#[derive(Debug, Deserialize)]
struct Subagent {
    thread_spawn: Option<ThreadSpawn>,
}
#[derive(Debug, Deserialize)]
struct ThreadSpawn {
    parent_thread_id: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct Context {
    pub turn_id: String,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub workspace_roots: Option<Vec<String>>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LimitWindow {
    pub window_minutes: Option<i64>,
    pub used_percent: Option<serde_json::Number>,
    pub resets_at: Option<i64>,
}
#[derive(Debug, Deserialize)]
struct Limits {
    limit_id: Option<String>,
    primary: Option<LimitWindow>,
    secondary: Option<LimitWindow>,
}
#[derive(Debug, Deserialize)]
struct EventPayload {
    r#type: String,
    rate_limits: Option<Limits>,
}
#[derive(Deserialize)]
struct Envelope<'a> {
    timestamp: Option<String>,
    r#type: String,
    #[serde(borrow)]
    payload: &'a RawValue,
}
pub enum Record {
    Metadata(Metadata),
    Context(Context),
    Usage {
        timestamp: String,
        usage: Usage,
    },
    Legacy {
        timestamp: Option<String>,
        windows: Vec<(Option<String>, String, LimitWindow)>,
    },
    Ignore,
    UnsupportedEnvelope,
}

pub fn decode(line: &[u8]) -> Result<Record, &'static str> {
    let envelope: Envelope = serde_json::from_slice(line).map_err(|_| "Malformed JSONL record")?;
    let raw = envelope.payload.get();
    match envelope.r#type.as_str() {
        "session_meta" => {
            let mut meta: Metadata =
                serde_json::from_str(raw).map_err(|_| "Invalid session metadata")?;
            if let Some(MetadataSource::Subagent { subagent }) = meta.source.take() {
                if let Some(parent) = subagent
                    .thread_spawn
                    .and_then(|spawn| spawn.parent_thread_id)
                {
                    meta.nested_parent_thread_id = Some(parent);
                }
            }
            if meta.id.is_empty() || meta.id.len() > 512 {
                return Err("Missing direct session identity");
            }
            Ok(Record::Metadata(meta))
        }
        "turn_context" => serde_json::from_str(raw)
            .map(Record::Context)
            .map_err(|_| "Invalid turn metadata"),
        "token_usage_record" => {
            let usage: Usage =
                serde_json::from_str(raw).map_err(|_| "Unsupported modern usage shape")?;
            let timestamp = envelope
                .timestamp
                .filter(|s| !s.is_empty() && s.len() <= 64)
                .ok_or("Missing usage timestamp")?;
            observation_time(&timestamp)?;
            if usage.thread_id.is_empty() || usage.thread_id.len() > 512 {
                return Err("Missing direct thread identity");
            }
            Ok(Record::Usage { timestamp, usage })
        }
        "event_msg" => {
            let event: EventPayload =
                serde_json::from_str(raw).map_err(|_| "Invalid event envelope")?;
            if event.r#type != "token_count" {
                return Ok(Record::Ignore);
            }
            let mut windows = Vec::new();
            if let Some(limits) = event.rate_limits {
                for (position, window) in
                    [("primary", limits.primary), ("secondary", limits.secondary)]
                {
                    if let Some(window) = window {
                        windows.push((limits.limit_id.clone(), position.into(), window));
                    }
                }
            }
            Ok(Record::Legacy {
                timestamp: envelope.timestamp,
                windows,
            })
        }
        "response_item" | "compacted" => Ok(Record::Ignore),
        _ => Ok(Record::UnsupportedEnvelope),
    }
}
