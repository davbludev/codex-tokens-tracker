//! Content is read on demand. SQLite retains neither messages nor raw-log copies.
//! Tokens and prices are never inferred from commands, text length or timing.
use crate::{
    adapter,
    calls::Window,
    source,
    storage::calls::open,
    weekly::{ReadError, Time},
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const SCAN_BYTES: usize = 4 * 1024 * 1024;
const MAX_LINE: usize = 2 * 1024 * 1024;
const PAGE: usize = 100;
const TEXT_CHUNK: usize = 64 * 1024;
const MAX_LINKS: usize = 4096;

#[derive(Default)]
pub struct Runtime(Mutex<Cache>);
#[derive(Default)]
struct Cache {
    sequence: u64,
    scans: BTreeMap<String, Scan>,
    texts: BTreeMap<String, TextSource>,
    text_order: VecDeque<String>,
}
impl Cache {
    fn key(&mut self, prefix: &str) -> String {
        self.sequence += 1;
        format!("{prefix}-{}", self.sequence)
    }
    fn text(&mut self, source: TextSource) -> String {
        let key = self.key("text");
        if self.text_order.len() >= 2048 {
            if let Some(old) = self.text_order.pop_front() {
                self.texts.remove(&old);
            }
        }
        self.text_order.push_back(key.clone());
        self.texts.insert(key.clone(), source);
        key
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub observation_id: String,
    pub start: Time,
    pub end: Time,
    pub cursor: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextQuery {
    pub text_ref: String,
    pub offset: Option<usize>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPage {
    pub text: String,
    pub next_offset: Option<usize>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub label: String,
    pub preview: String,
    pub text_ref: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub time: Time,
    pub kind: String,
    pub label: String,
    pub association: &'static str,
    pub parent_id: Option<String>,
    pub fields: Vec<Field>,
    pub status: Option<String>,
    pub paths: Vec<String>,
    pub paths_inferred: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub events: Vec<Event>,
    pub next_cursor: Option<String>,
    pub scanned_bytes: u64,
    pub total_bytes: u64,
    pub complete: bool,
    pub association: &'static str,
    pub notices: Vec<String>,
}

struct Anchor {
    database: PathBuf,
    path: PathBuf,
    generation: i64,
    identity: String,
    size: u64,
    offset: u64,
    hash: Vec<u8>,
    usage: adapter::Usage,
}
impl Anchor {
    fn create(database: &Path, id: &str, window: Window) -> Result<Arc<Self>, String> {
        let id = id
            .parse::<i64>()
            .ok()
            .filter(|id| *id > 0)
            .ok_or("Invalid invocation")?;
        let connection = open(database).map_err(|_| "Accounting database unavailable")?;
        let found = connection.query_row("SELECT o.source_path,o.source_generation,o.source_offset,o.normalized,o.time_seconds,o.time_nanos,s.generation,s.identity FROM observations o LEFT JOIN sources s ON s.path=o.source_path WHERE o.id=? AND o.accepted=1", [id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,i64>(2)?,row.get::<_,String>(3)?,row.get::<_,i64>(4)?,row.get::<_,u32>(5)?,row.get::<_,Option<i64>>(6)?,row.get::<_,Option<String>>(7)?))).optional().map_err(|_| "Invocation source unavailable")?.ok_or("Invocation no longer retained")?;
        let (
            path,
            generation,
            offset,
            normalized,
            seconds,
            nanos,
            current_generation,
            expected_identity,
        ) = found;
        let offset = u64::try_from(offset).map_err(|_| "Invalid source offset")?;
        if !window.contains(Time { seconds, nanos }) {
            return Err("Invocation is outside the selected interval".into());
        }
        let usage: adapter::Usage =
            serde_json::from_str(&normalized).map_err(|_| "Saved invocation is invalid")?;
        if [
            &usage.session_id,
            &usage.turn_id,
            &usage.root_turn_id,
            &usage.response_id,
        ]
        .iter()
        .any(|value| value.as_ref().is_some_and(|value| value.len() > 1024))
        {
            return Err(
                "Activity identifiers exceed inspection limits; saved accounting remains available"
                    .into(),
            );
        }
        let mut candidates = Vec::new();
        if current_generation == Some(generation) {
            candidates.push((path.clone(), generation, expected_identity));
        }
        // Archive moves retain accepted provenance. A registered duplicate can
        // supply the exact same record at the same offset; never guess a path.
        let mut statement = connection.prepare("SELECT path,generation,identity FROM sources WHERE thread_id=?1 AND path<>?2 ORDER BY path LIMIT 16").map_err(|_| "Source metadata unavailable")?;
        let alternatives = statement
            .query_map(rusqlite::params![usage.thread_id, path], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|_| "Source metadata unavailable")?;
        candidates.extend(
            alternatives
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|_| "Source metadata unavailable")?,
        );
        for (candidate, generation, expected) in candidates {
            let path = PathBuf::from(candidate);
            let Ok(mut file) = source::shared_open(&path) else {
                continue;
            };
            let Ok(identity) = source::file_identity(&file) else {
                continue;
            };
            if expected.is_some_and(|expected| expected != identity) {
                continue;
            }
            let Ok(metadata) = file.metadata() else {
                continue;
            };
            let size = metadata.len();
            let Ok(bytes) = record(&mut file, offset, size) else {
                continue;
            };
            match adapter::decode(&bytes) {
                Ok(adapter::Record::Usage { usage: actual, .. }) if actual == usage => (),
                _ => continue,
            }
            return Ok(Arc::new(Self {
                database: database.into(),
                path,
                generation,
                identity,
                size,
                offset,
                hash: Sha256::digest(&bytes).to_vec(),
                usage,
            }));
        }
        Err("Source log is missing, replaced, or no longer matches this invocation; saved accounting remains available".into())
    }
    fn file(&self) -> Result<File, String> {
        let connection = open(&self.database).map_err(|_| "Accounting database unavailable")?;
        let generation = connection
            .query_row(
                "SELECT generation FROM sources WHERE path=?",
                [self.path.to_string_lossy().as_ref()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| "Source metadata unavailable")?;
        if generation != Some(self.generation) {
            return Err("Source generation changed; reopen the invocation".into());
        }
        let mut file =
            source::shared_open(&self.path).map_err(|_| "Source log is missing or unreadable")?;
        if source::file_identity(&file).map_err(|_| "Source identity unavailable")? != self.identity
            || file
                .metadata()
                .map_err(|_| "Source metadata unavailable")?
                .len()
                < self.size
        {
            return Err("Source was replaced or truncated; reopen the invocation".into());
        }
        if Sha256::digest(record(&mut file, self.offset, self.size)?).as_slice() != self.hash {
            return Err("Source usage anchor changed; reopen the invocation".into());
        }
        Ok(file)
    }
}
fn record(file: &mut File, offset: u64, size: u64) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| "Cannot seek source record")?;
    let mut bytes = Vec::new();
    BufReader::new(file.take(size.saturating_sub(offset).min((MAX_LINE + 1) as u64)))
        .read_until(b'\n', &mut bytes)
        .map_err(|_| "Cannot read source record")?;
    if bytes.len() > MAX_LINE || bytes.last() != Some(&b'\n') {
        return Err("Source record is oversized or incomplete".into());
    }
    Ok(bytes)
}

#[derive(Clone)]
struct TextSource {
    anchor: Arc<Anchor>,
    offset: u64,
    hash: Vec<u8>,
    pointer: String,
}
fn display(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.into();
    }
    if let Some(items) = value.as_array() {
        if items
            .iter()
            .all(|item| item.get("text").is_some_and(Value::is_string))
        {
            return items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    serde_json::to_string_pretty(value).unwrap_or_default()
}

struct Scan {
    observation: String,
    window: Window,
    anchor: Arc<Anchor>,
    offset: u64,
    tail: Vec<u8>,
    tail_start: u64,
    discarding: bool,
    locating: bool,
    finished: bool,
    next_usage: bool,
    batch_start: u64,
    batch_turn: Option<String>,
    turn: Option<String>,
    opening_known: bool,
    output_seen: bool,
    ambiguous: bool,
    generated: BTreeSet<String>,
    open_tools: BTreeSet<String>,
    other_tools: BTreeSet<String>,
    seen: BTreeSet<String>,
    excluded: bool,
    notices: BTreeSet<String>,
}
impl Scan {
    fn new(observation: String, window: Window, anchor: Arc<Anchor>) -> Self {
        Self {
            observation,
            window,
            anchor,
            offset: 0,
            tail: Vec::new(),
            tail_start: 0,
            discarding: false,
            locating: true,
            finished: false,
            next_usage: false,
            batch_start: 0,
            batch_turn: None,
            turn: None,
            opening_known: false,
            output_seen: false,
            ambiguous: false,
            generated: BTreeSet::new(),
            open_tools: BTreeSet::new(),
            other_tools: BTreeSet::new(),
            seen: BTreeSet::new(),
            excluded: false,
            notices: BTreeSet::new(),
        }
    }
    fn association(&self) -> &'static str {
        if self.ambiguous || !self.opening_known {
            "ambiguous"
        } else {
            "logOrder"
        }
    }
    fn inspect(&mut self, bytes: &[u8], offset: u64, cache: &mut Cache) -> Option<Event> {
        let value: Value = match serde_json::from_slice(bytes) {
            Ok(value) => value,
            Err(_) => {
                self.notices.insert(
                    "Malformed records were skipped; activity coverage is incomplete".into(),
                );
                return None;
            }
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let payload = value.get("payload").unwrap_or(&Value::Null);
        let payload_kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "turn_context" || (kind == "event_msg" && payload_kind == "task_started") {
            self.turn = string(payload, "turn_id");
            if self.locating && self.batch_start == 0 {
                self.batch_start = offset;
                self.batch_turn = self.turn.clone();
                self.opening_known = self.turn == self.anchor.usage.turn_id && self.turn.is_some();
            }
        }
        let same_turn = string(payload, "turn_id")
            .or_else(|| string(payload, "turnId"))
            .or_else(|| self.turn.clone())
            == self.anchor.usage.turn_id;
        let generated = kind == "response_item"
            && (matches!(
                payload_kind,
                "function_call" | "custom_tool_call" | "reasoning"
            ) || (payload_kind == "message"
                && payload.get("role").and_then(Value::as_str) == Some("assistant")));
        let output = kind == "response_item"
            && matches!(
                payload_kind,
                "function_call_output" | "custom_tool_call_output"
            );
        if self.locating {
            if offset == self.anchor.offset {
                self.locating = false;
                self.offset = self.batch_start;
                self.tail.clear();
                self.turn = self.batch_turn.clone();
                self.generated.clear();
                if !self.opening_known {
                    self.notices.insert("Opening call boundary is unavailable; activity is not assigned with certainty".into());
                }
                if self.ambiguous {
                    self.notices.insert("Multiple generation phases share a usage boundary; their activity is unassigned".into());
                }
            } else if kind == "token_usage_record" {
                self.batch_start = offset + bytes.len() as u64;
                self.batch_turn = self.turn.clone();
                self.opening_known = same_turn;
                self.output_seen = false;
                self.ambiguous = false;
                self.generated.clear();
            } else if same_turn {
                if generated && self.output_seen {
                    self.ambiguous = true;
                }
                let id = string(payload, "call_id").or_else(|| string(payload, "id"));
                if output {
                    self.output_seen |= id.as_ref().is_some_and(|id| self.generated.contains(id));
                }
                if generated && self.generated.len() < MAX_LINKS {
                    if let Some(id) = id {
                        self.generated.insert(id);
                    }
                }
            }
            return None;
        }
        if kind == "token_usage_record" && offset > self.anchor.offset {
            self.next_usage = true;
            // All generated tools have their results. Later unrelated calls do
            // not require reading the rest of a long conversation.
            if self.open_tools.is_empty() {
                self.finished = true;
            }
            return None;
        }
        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|value| adapter::observation_time(value).ok())
            .map(|(seconds, nanos)| Time { seconds, nanos });
        let item = payload.get("item").unwrap_or(payload);
        let item_kind = item
            .get("type")
            .and_then(Value::as_str)
            .filter(|value| value.len() <= 128)
            .unwrap_or("");
        let id = string(payload, "call_id")
            .or_else(|| string(item, "id"))
            .unwrap_or_else(|| format!("record-{offset}"));
        if generated
            && offset > self.anchor.offset
            && same_turn
            && matches!(payload_kind, "function_call" | "custom_tool_call")
            && self.other_tools.len() < MAX_LINKS
        {
            self.other_tools.insert(id.clone());
        }
        if output {
            self.other_tools.remove(&id);
        }
        let mut association = self.association();
        let mut parent = None;
        let mut base = "/payload";
        let mut pointers: Vec<(&str, &str)> = Vec::new();
        let label;
        let event_kind;
        if generated && offset < self.anchor.offset && same_turn {
            if self.generated.len() >= MAX_LINKS {
                self.notices.insert(
                    "This call exceeds the activity-link budget; remaining items are unassigned"
                        .into(),
                );
                association = "unassigned";
            } else {
                self.generated.insert(id.clone());
            }
            if matches!(payload_kind, "function_call" | "custom_tool_call") {
                self.open_tools.insert(id.clone());
            }
            event_kind = payload_kind.to_owned();
            label = string(payload, "name").unwrap_or_else(|| {
                match payload_kind {
                    "reasoning" => "Available reasoning summary",
                    "message" => "Assistant message",
                    _ => "Tool call",
                }
                .into()
            });
            pointers.extend([
                ("Arguments", "arguments"),
                ("Input", "input"),
                ("Text", "content"),
                ("Summary", "summary"),
            ]);
        } else if output && self.generated.contains(&id) {
            self.open_tools.remove(&id);
            parent = Some(id.clone());
            association = "itemId";
            event_kind = "toolOutput".into();
            label = "Tool result".into();
            pointers.push(("Output", "output"));
        } else if kind == "event_msg" && payload_kind == "item_completed" && same_turn {
            base = if payload.get("item").is_some() {
                "/payload/item"
            } else {
                "/payload"
            };
            if self.generated.contains(&id) {
                // Messages/reasoning already have their source representation. Tool
                // completions add status/output, rather than repeating the call text.
                if matches!(
                    item_kind,
                    "AgentMessage" | "Reasoning" | "agent_message" | "reasoning"
                ) {
                    return None;
                }
                parent = Some(id.clone());
                association = "itemId";
            } else if self.open_tools.len() == 1 && self.other_tools.is_empty() {
                parent = self.open_tools.iter().next().cloned();
                association = "activeBatch";
            } else if !self.next_usage || !self.open_tools.is_empty() {
                association = "unassigned";
            } else {
                return None;
            }
            event_kind = item_kind.to_owned();
            label = match item_kind {
                "CommandExecution" => "Command",
                "FileChange" => "File changes",
                "ImageView" => "Viewed image",
                _ => item_kind,
            }
            .to_owned();
            pointers.extend([
                ("Command", "command"),
                ("Working directory", "cwd"),
                ("Output", "output"),
                ("Output", "aggregated_output"),
                ("Changes", "changes"),
                ("Path", "path"),
                ("Details", "content"),
                ("Text", "text"),
                ("Result", "result"),
            ]);
        } else if kind == "compacted"
            && payload
                .get("compaction_response_id")
                .and_then(Value::as_str)
                == self.anchor.usage.response_id.as_deref()
            && self.anchor.usage.response_id.is_some()
        {
            association = "responseId";
            event_kind = "compaction".into();
            label = "Context compaction".into();
            pointers.extend([("Summary", "message"), ("Summary", "summary")]);
        } else if same_turn
            && offset < self.anchor.offset
            && ((kind == "response_item" && matches!(payload_kind, "agent_message" | "message"))
                || (kind == "event_msg" && payload_kind == "user_message"))
        {
            association = "context";
            event_kind = "context".into();
            label = "Incoming message / call context".into();
            pointers.extend([
                ("Message", "message"),
                ("Text", "content"),
                ("Text", "text"),
            ]);
        } else {
            return None;
        }
        // Usage timestamps select calls; activity timestamps select their contents.
        // In particular a command completion cannot reveal a later tool result.
        let Some(time) = timestamp else {
            self.notices.insert(
                "Some related activity has no timestamp and cannot be assigned to the selection"
                    .into(),
            );
            return None;
        };
        if !self.window.contains(time) {
            self.excluded = true;
            return None;
        }
        let start_ms = payload
            .get("started_at_ms")
            .or_else(|| item.get("started_at_ms"))
            .and_then(Value::as_i64);
        if start_ms.is_some_and(|ms| {
            !self.window.contains(Time {
                seconds: ms.div_euclid(1000),
                nanos: (ms.rem_euclid(1000) as u32) * 1_000_000,
            })
        }) {
            self.excluded = true;
            if item_kind == "CommandExecution" {
                pointers.retain(|(_, field)| !matches!(*field, "command" | "cwd"));
            }
        }
        let unique = format!("{event_kind}:{id}:{base}");
        if self.seen.len() < MAX_LINKS && !self.seen.insert(unique) {
            return None;
        }
        let data = if base == "/payload/item" {
            item
        } else {
            payload
        };
        let mut fields = Vec::new();
        for (label, field) in pointers {
            if let Some(body) = data.get(field).filter(|body| !body.is_null()) {
                let text = display(body);
                if text.is_empty() {
                    continue;
                }
                let key = cache.text(TextSource {
                    anchor: self.anchor.clone(),
                    offset,
                    hash: Sha256::digest(bytes).to_vec(),
                    pointer: format!("{base}/{field}"),
                });
                fields.push(Field {
                    label: label.into(),
                    preview: text.chars().take(400).collect(),
                    text_ref: key,
                });
            }
        }
        let mut paths = Vec::new();
        let mut paths_inferred = false;
        if let Some(changes) = data.get("changes").and_then(Value::as_object) {
            paths.extend(
                changes
                    .keys()
                    .filter(|path| path.len() <= 4096)
                    .take(100)
                    .cloned(),
            );
        }
        if let Some(path) = data
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| path.len() <= 4096)
        {
            paths.push(path.into());
        }
        if item_kind == "CommandExecution"
            && !start_ms.is_some_and(|ms| {
                !self.window.contains(Time {
                    seconds: ms.div_euclid(1000),
                    nanos: (ms.rem_euclid(1000) as u32) * 1_000_000,
                })
            })
        {
            if let Some(command) = data.get("command").and_then(Value::as_str) {
                paths = inferred_paths(command);
                paths_inferred = !paths.is_empty();
            }
        }
        Some(Event {
            id: format!("event-{offset}-{event_kind}"),
            time,
            kind: event_kind,
            label,
            association,
            parent_id: parent,
            fields,
            status: string(item, "status")
                .or_else(|| item.get("exit_code").map(|value| format!("Exit {value}"))),
            paths,
            paths_inferred,
        })
    }
    fn advance(&mut self, cache: &mut Cache) -> Result<Vec<Event>, String> {
        let mut file = self.anchor.file()?;
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|_| "Cannot seek source log")?;
        let mut reader = BufReader::new(file);
        let mut events = Vec::new();
        let mut budget = SCAN_BYTES;
        while budget > 0 && self.offset < self.anchor.size && events.len() < PAGE && !self.finished
        {
            if self.tail.is_empty() && !self.discarding {
                self.tail_start = self.offset;
            }
            let mut piece = Vec::new();
            let cap = budget
                .min(64 * 1024)
                .min((self.anchor.size - self.offset) as usize);
            let count = reader
                .by_ref()
                .take(cap as u64)
                .read_until(b'\n', &mut piece)
                .map_err(|_| "Cannot read source log")?;
            if count == 0 {
                break;
            }
            budget -= count;
            self.offset += count as u64;
            if !self.discarding {
                if self.tail.len() + count > MAX_LINE {
                    self.tail.clear();
                    self.discarding = true;
                    self.notices
                        .insert("Oversized log records were skipped".into());
                } else {
                    self.tail.extend_from_slice(&piece);
                }
            }
            if piece.last() == Some(&b'\n') {
                if !self.discarding {
                    let bytes = std::mem::take(&mut self.tail);
                    let locating = self.locating;
                    if let Some(event) = self.inspect(&bytes, self.tail_start, cache) {
                        events.push(event);
                    }
                    if locating && !self.locating {
                        reader
                            .seek(SeekFrom::Start(self.offset))
                            .map_err(|_| "Cannot seek call boundary")?;
                    }
                }
                self.discarding = false;
            }
        }
        if self.offset == self.anchor.size && !self.tail.is_empty() {
            self.notices
                .insert("The last log record is incomplete".into());
        }
        if self.excluded {
            self.notices.insert(
                "Some related activity is outside the selected interval and is hidden".into(),
            );
        }
        Ok(events)
    }
}
fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| value.len() <= 1024)
        .map(str::to_owned)
}
fn inferred_paths(command: &str) -> Vec<String> {
    // Only explicit path-shaped command arguments. This is intent, not an audit
    // of files a script actually accessed; never execute a logged command.
    command
        .split_whitespace()
        .map(|part| part.trim_matches(['\'', '"', ',', ';', '(', ')']))
        .filter(|part| {
            part.len() < 512
                && !part.contains("://")
                && (part.contains('/') || part.contains('\\'))
                && !part.starts_with('-')
        })
        .take(30)
        .map(str::to_owned)
        .collect()
}

impl Runtime {
    pub fn activity(&self, database: &Path, query: Query) -> Result<Page, ReadError> {
        let window = Window {
            start: query.start,
            end: query.end,
        };
        window.validate()?;
        let mut cache = self.0.lock().map_err(|_| ReadError::Storage)?;
        let failure = |message: String| Page {
            events: Vec::new(),
            next_cursor: None,
            scanned_bytes: 0,
            total_bytes: 0,
            complete: false,
            association: "unavailable",
            notices: vec![message],
        };
        let (key, mut scan) = if let Some(key) = query.cursor {
            let scan = cache.scans.remove(&key).ok_or(ReadError::InvalidQuery)?;
            if scan.observation != query.observation_id || scan.window != window {
                return Err(ReadError::InvalidQuery);
            }
            (key, scan)
        } else {
            let anchor = match Anchor::create(database, &query.observation_id, window) {
                Ok(anchor) => anchor,
                Err(message) => return Ok(failure(message)),
            };
            (
                cache.key("scan"),
                Scan::new(query.observation_id, window, anchor),
            )
        };
        let events = match scan.advance(&mut cache) {
            Ok(events) => events,
            Err(message) => return Ok(failure(message)),
        };
        let more = !scan.finished && scan.offset < scan.anchor.size;
        let result = Page {
            events,
            next_cursor: more.then(|| key.clone()),
            scanned_bytes: scan.offset,
            total_bytes: scan.anchor.size,
            complete: !more && scan.notices.is_empty(),
            association: scan.association(),
            notices: scan.notices.iter().cloned().collect(),
        };
        if more {
            if cache.scans.len() >= 8 {
                if let Some(old) = cache.scans.keys().next().cloned() {
                    cache.scans.remove(&old);
                }
            }
            cache.scans.insert(key, scan);
        }
        Ok(result)
    }
    pub fn text(&self, query: TextQuery) -> Result<TextPage, String> {
        let source = self
            .0
            .lock()
            .map_err(|_| "Activity cache unavailable")?
            .texts
            .get(&query.text_ref)
            .cloned()
            .ok_or("Text reference expired; reopen the invocation")?;
        let mut file = source.anchor.file()?;
        let bytes = record(&mut file, source.offset, source.anchor.size)?;
        if Sha256::digest(&bytes).as_slice() != source.hash {
            return Err("This activity record changed; reopen the invocation".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Activity record is invalid")?;
        let text = display(
            value
                .pointer(&source.pointer)
                .ok_or("Activity field is no longer available")?,
        );
        let offset = query.offset.unwrap_or(0);
        if offset > text.len() || !text.is_char_boundary(offset) {
            return Err("Invalid text continuation".into());
        }
        let mut end = (offset + TEXT_CHUNK).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Ok(TextPage {
            text: text[offset..end].into(),
            next_offset: (end < text.len()).then_some(end),
        })
    }
}
