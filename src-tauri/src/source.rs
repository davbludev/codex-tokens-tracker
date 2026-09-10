use crate::{
    adapter,
    storage::{InputLine, Result, Store, MAX_BATCH_LINES},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_LINE: usize = 2 * 1024 * 1024;
const READ_BUDGET: usize = 256 * 1024;

pub fn sessions_directory() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|p| PathBuf::from(p).join(".codex"))
        })
        .map(|p| p.join("sessions"))
}
pub fn is_rollout(path: &Path) -> bool {
    path.extension().is_some_and(|v| v == "jsonl")
        && path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("rollout-"))
}

/// Normalize separator and Windows extended-prefix spelling even after removal,
/// when filesystem canonicalization is no longer available.
pub fn normalized_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(text) = path.to_str() {
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}")).components().collect();
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest).components().collect();
        }
    }
    path.components().collect()
}

/// Failed access is not evidence of disappearance. Only NotFound clears a tail.
pub fn reconcile_presence(
    store: &Store,
    path: &str,
    generation: i64,
    presence: std::io::Result<()>,
) -> Result<()> {
    match presence {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            store.source_removed(path, generation)
        }
        Err(error) => Err(error.into()),
    }
}
/// Directory iterators retain only the current ancestry, never a whole tree.
/// Symlinks are excluded so discovery stays within the selected source trees.
pub struct Discovery {
    roots: Vec<PathBuf>,
    stack: Vec<fs::ReadDir>,
}

pub struct Discovered {
    pub paths: Vec<PathBuf>,
    pub failed: bool,
}

impl Discovery {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            stack: Vec::new(),
        }
    }

    pub fn pending(&self) -> bool {
        !self.roots.is_empty() || !self.stack.is_empty()
    }

    pub fn step(&mut self) -> Discovered {
        let mut result = Discovered {
            paths: Vec::new(),
            failed: false,
        };
        for _ in 0..64 {
            if let Some(entries) = self.stack.last_mut() {
                match entries.next() {
                    Some(Ok(entry)) => match entry.file_type() {
                        Ok(kind) if kind.is_dir() => {
                            if self.stack.len() >= 128 {
                                result.failed = true;
                            } else {
                                match fs::read_dir(entry.path()) {
                                    Ok(entries) => self.stack.push(entries),
                                    Err(_) => result.failed = true,
                                }
                            }
                        }
                        Ok(kind) if kind.is_file() && is_rollout(&entry.path()) => {
                            result.paths.push(entry.path())
                        }
                        Ok(_) => (),
                        Err(_) => result.failed = true,
                    },
                    Some(Err(_)) => result.failed = true,
                    None => {
                        self.stack.pop();
                    }
                }
            } else if let Some(root) = self.roots.pop() {
                match fs::symlink_metadata(&root) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                        match fs::read_dir(root) {
                            Ok(entries) => self.stack.push(entries),
                            Err(_) => result.failed = true,
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                    Ok(_) => (),
                    Err(_) => result.failed = true,
                }
            } else {
                break;
            }
        }
        result
    }
}
fn shared_open(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x1 | 0x2 | 0x4); // Reader, writer, and rename/delete sharing.
    }
    options.open(path)
}

fn file_identity(file: &File) -> std::io::Result<String> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // The handle remains owned by File; the API writes exactly this structure.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(format!(
            "windows:{}:{}:{}",
            information.dwVolumeSerialNumber, information.nFileIndexHigh, information.nFileIndexLow
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(not(any(windows, unix)))]
    {
        Ok(format!("created:{:?}", file.metadata()?.created()?))
    }
}

/// Fingerprint the fixed prefix and the last observed boundary (at most 8 KiB).
/// The prefix length is derived from the saved boundary, so appends cannot change it.
fn fingerprint(file: &mut File, start: u64, length: u32) -> std::io::Result<[u8; 32]> {
    let mut digest = Sha256::new();
    let mut bytes = [0u8; 4096];
    let prefix = start.min(4096) as usize;
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut bytes[..prefix])?;
    digest.update(&bytes[..prefix]);
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut bytes[..length as usize])?;
    digest.update(&bytes[..length as usize]);
    Ok(digest.finalize().into())
}

/// One cooperative unit. Returns true only when captured bytes remain to read.
/// Tail bytes are reconstructed from the source, never loaded from the database.
pub fn ingest_batch(store: &mut Store, path: &Path) -> Result<bool> {
    let path = normalized_path(path);
    let key = path.to_string_lossy();
    let mut state = store.source_state(&key)?;
    let mut file = match shared_open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            store.source_removed(&key, state.generation)?;
            return Ok(false);
        }
        Err(error) => {
            store.source_status(
                &key,
                state.progress.tail_length > 0,
                Some("Source could not be read; confirmed usage retained"),
            )?;
            return Err(error.into());
        }
    };
    let size = file.metadata()?.len();
    let identity = file_identity(&file)?;
    let previous = &state.progress;
    let verification_failed = match previous.verification_hash {
        Some(expected) => fingerprint(
            &mut file,
            previous.verification_start,
            previous.verification_length,
        )
        .map_or(true, |actual| expected != actual),
        None => previous.offset > 0 || previous.tail_length > 0,
    };
    if state.identity.as_ref().is_some_and(|old| old != &identity)
        || size < previous.known_size
        || verification_failed
    {
        store.restart_source(&key, state.generation, Some(&identity), size)?;
        state = store.source_state(&key)?;
    } else if state.identity.is_none() {
        store.initialize_source_identity(&key, state.generation, &identity, size)?;
    }
    let mut progress = state.progress;
    let mut tail = Vec::new();
    if !progress.tail_discarding && progress.tail_length > 0 {
        // A normal unfinished record is bounded by MAX_LINE; oversized tails resume
        // at their saved scanned boundary without re-reading discarded bytes.
        if progress.tail_length > MAX_LINE as u64 {
            return Err(crate::storage::Error::RecoveryMetadata);
        }
        tail.resize(progress.tail_length as usize, 0);
        file.seek(SeekFrom::Start(progress.offset))?;
        file.read_exact(&mut tail)?;
    }
    let mut cursor = progress.offset + progress.tail_length;
    file.seek(SeekFrom::Start(cursor))?;
    let mut bytes = vec![0; READ_BUDGET.min(size.saturating_sub(cursor) as usize)];
    file.read_exact(&mut bytes)?;
    let mut lines = Vec::new();
    for segment in bytes.split_inclusive(|byte| *byte == b'\n') {
        cursor += segment.len() as u64;
        progress.tail_length += segment.len() as u64;
        if !progress.tail_discarding {
            if tail.len() + segment.len() <= MAX_LINE {
                tail.extend_from_slice(segment);
            } else {
                tail.clear();
                progress.tail_discarding = true;
            }
        }
        if segment.last() == Some(&b'\n') {
            progress.ordinal = progress
                .ordinal
                .checked_add(1)
                .ok_or(crate::storage::Error::Offset)?;
            lines.push(InputLine {
                start: progress.offset,
                end: cursor,
                ordinal: progress.ordinal,
                record: if progress.tail_discarding {
                    Ok(adapter::Record::Unreadable)
                } else {
                    adapter::decode(&tail)
                },
            });
            progress.offset = cursor;
            progress.tail_length = 0;
            progress.tail_discarding = false;
            tail.clear();
            if lines.len() == MAX_BATCH_LINES {
                break;
            }
        }
    }
    progress.known_size = size;
    progress.verification_length = cursor.min(4096) as u32;
    progress.verification_start = cursor - u64::from(progress.verification_length);
    progress.verification_hash = if cursor == 0 {
        None
    } else {
        Some(fingerprint(
            &mut file,
            progress.verification_start,
            progress.verification_length,
        )?)
    };
    store.batch(&key, state.generation, lines, progress)?;
    Ok(cursor < size)
}

#[cfg(test)]
pub fn ingest(store: &mut Store, path: &Path) -> Result<()> {
    while ingest_batch(store, path)? {}
    while store.reconcile_pending()? {}
    Ok(())
}
