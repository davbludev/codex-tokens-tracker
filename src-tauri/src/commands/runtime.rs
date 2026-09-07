use crate::{
    source::{self, Discovery},
    storage::{Result, Store},
};
use notify::{event::ModifyKind, Event, EventKind};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const DEBOUNCE: Duration = Duration::from_millis(75);
const QUEUE_LIMIT: usize = 512;

/// Bounded cooperative units: directory entries, one file batch, then promotion.
/// Only concrete source events schedule recovery; an idle coordinator has no work.
pub struct Work {
    pub roots: Vec<PathBuf>,
    discovery: VecDeque<Discovery>,
    reads: VecDeque<PathBuf>,
    queued: HashSet<PathBuf>,
    debounce: HashMap<PathBuf, Instant>,
    reconcile: bool,
    lane: usize,
    recovery: bool,
    recovery_notice: bool,
    sweep: Option<SourceSweep>,
    sweep_requested: bool,
    pub discovered: u64,
    pub batches: u64,
    pub diagnostic: Option<String>,
}

struct SourceSweep {
    after: Option<String>,
    through: String,
}

impl Work {
    pub fn new(home: &Path) -> Self {
        let home = source::normalized_path(home);
        let roots = vec![home.join("sessions"), home.join("archived_sessions")];
        Self {
            discovery: VecDeque::from([Discovery::new(roots.clone())]),
            roots,
            reads: VecDeque::new(),
            queued: HashSet::new(),
            debounce: HashMap::new(),
            reconcile: true,
            lane: 0,
            recovery: false,
            recovery_notice: false,
            sweep: None,
            sweep_requested: true,
            discovered: 0,
            batches: 0,
            diagnostic: None,
        }
    }

    pub fn recover(&mut self, message: &'static str) {
        self.diagnostic = Some(message.into());
        self.recovery_notice = true;
        // Collapse overflow/error storms into one follow-up discovery pass.
        self.recovery = true;
        self.sweep_requested = true;
    }

    pub fn failed(&mut self, message: String) {
        self.diagnostic = Some(message);
        self.recovery_notice = false;
    }

    fn enqueue(&mut self, path: PathBuf) {
        if self.queued.contains(&path) {
            return;
        }
        if self.reads.len() >= QUEUE_LIMIT {
            self.recover("Source queue filled; recovering available files");
        } else {
            self.queued.insert(path.clone());
            self.reads.push_back(path);
        }
    }

    pub fn event(&mut self, event: Event, now: Instant) {
        if event.need_rescan() {
            self.recover("Native watcher requested source recovery");
        }
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        if event.paths.len() > QUEUE_LIMIT {
            self.recover("Source event exceeded queue capacity; recovering available files");
        }
        for path in event.paths.into_iter().take(QUEUE_LIMIT) {
            let path = source::normalized_path(&path);
            if !self.roots.iter().any(|root| path.starts_with(root)) {
                continue;
            }
            if source::is_rollout(&path) {
                if self.debounce.len() >= QUEUE_LIMIT && !self.debounce.contains_key(&path) {
                    self.recover("Source queue filled; recovering available files");
                } else {
                    // Preserve the first deadline: sustained writes cannot starve reads.
                    self.debounce.entry(path).or_insert(now + DEBOUNCE);
                }
            } else {
                if matches!(
                    event.kind,
                    EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
                ) {
                    self.sweep_requested = true;
                }
                if matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
                ) && path.is_dir()
                {
                    if self.discovery.len() < 8 {
                        self.discovery.push_back(Discovery::new(vec![path]));
                    } else {
                        self.recover("Directory event burst; recovering available files");
                    }
                }
            }
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.debounce.values().copied().min()
    }

    pub fn busy(&self) -> bool {
        !self.discovery.is_empty()
            || !self.reads.is_empty()
            || self.reconcile
            || self.recovery
            || self.sweep.is_some()
            || self.sweep_requested
    }

    pub fn progress(&self) -> String {
        let phase = if self.busy() {
            "Importing / recovering history"
        } else if self.roots.iter().any(|path| path.is_dir()) {
            "Live monitoring"
        } else {
            "Waiting for Codex source directories"
        };
        format!(
            "{phase} · {} files discovered · {} read batches · {} queued files",
            self.discovered,
            self.batches,
            self.reads.len() + self.debounce.len()
        )
    }

    pub fn step(&mut self, store: &mut Store, now: Instant) -> Result<bool> {
        let due: Vec<_> = self
            .debounce
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(path, _)| path.clone())
            .collect();
        for path in due {
            self.debounce.remove(&path);
            self.enqueue(path);
        }
        if self.recovery && self.discovery.is_empty() && self.reads.is_empty() {
            self.discovery.push_back(Discovery::new(self.roots.clone()));
            self.recovery = false;
        }
        for _ in 0..4 {
            let lane = self.lane;
            self.lane = (self.lane + 1) % 4;
            match lane {
                0 if !self.discovery.is_empty() && self.reads.len() <= QUEUE_LIMIT - 64 => {
                    let discovery = self.discovery.front_mut().unwrap();
                    let found = discovery.step();
                    if !discovery.pending() {
                        self.discovery.pop_front();
                    }
                    if found.failed {
                        self.failed("Some source directories could not be read".into());
                    }
                    self.discovered = self.discovered.saturating_add(found.paths.len() as u64);
                    for path in found.paths {
                        self.enqueue(path);
                    }
                    return Ok(true);
                }
                1 if !self.reads.is_empty() => {
                    let path = self.reads.pop_front().unwrap();
                    self.queued.remove(&path);
                    match source::ingest_batch(store, &path) {
                        Ok(more) => {
                            if more {
                                self.enqueue(path);
                            }
                        }
                        Err(error) => {
                            self.failed(error.to_string());
                        }
                    }
                    self.batches = self.batches.saturating_add(1);
                    self.reconcile = true;
                    return Ok(true);
                }
                2 if self.reconcile => {
                    self.reconcile = store.reconcile_pending()?;
                    return Ok(true);
                }
                3 if self.sweep_requested || self.sweep.is_some() => {
                    if self.sweep.is_none() {
                        self.sweep = store.source_watermark()?.map(|through| SourceSweep {
                            after: None,
                            through,
                        });
                        self.sweep_requested = false;
                    }
                    if let Some(sweep) = &self.sweep {
                        let page = store.source_page(sweep.after.as_deref(), &sweep.through)?;
                        for (path, generation) in &page {
                            let normalized = source::normalized_path(Path::new(path));
                            if self.roots.iter().any(|root| normalized.starts_with(root)) {
                                match source::reconcile_presence(store, path, *generation, std::fs::metadata(&normalized).map(|_| ())) {
                                    Err(crate::storage::Error::Io(_)) => self.failed("Some tracked sources could not be checked; confirmed usage retained".into()),
                                    Err(error) => return Err(error),
                                    Ok(()) => (),
                                }
                            }
                        }
                        if page.len() < 64 {
                            self.sweep = None;
                        } else {
                            self.sweep.as_mut().unwrap().after =
                                page.last().map(|(path, _)| path.clone());
                        }
                    }
                    return Ok(true);
                }
                _ => (),
            }
        }
        if self.recovery_notice && !self.busy() && self.debounce.is_empty() {
            self.diagnostic = None;
            self.recovery_notice = false;
            return Ok(true);
        }
        Ok(false)
    }
}
