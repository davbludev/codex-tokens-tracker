use crate::{
    adapter,
    storage::{Result, Store},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

const MAX_LINE: usize = 2 * 1024 * 1024;

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
pub fn latest_rollout(directory: &Path) -> std::io::Result<Option<PathBuf>> {
    fn visit(dir: &Path, latest: &mut Option<(SystemTime, PathBuf)>) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                visit(&entry.path(), latest)?;
            } else if kind.is_file() && is_rollout(&entry.path()) {
                let modified = entry.metadata()?.modified()?;
                if latest
                    .as_ref()
                    .is_none_or(|(time, path)| (modified, entry.path()) > (*time, path.clone()))
                {
                    *latest = Some((modified, entry.path()));
                }
            }
        }
        Ok(())
    }
    let mut latest = None;
    visit(directory, &mut latest)?;
    Ok(latest.map(|(_, path)| path))
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

pub fn ingest(store: &mut Store, path: &Path) -> Result<()> {
    let identity = path.to_string_lossy();
    let (mut offset, mut ordinal) = store.checkpoint(&identity)?;
    let mut file = shared_open(path)?;
    if file.metadata()?.len() < offset {
        store.source_status(
            &identity,
            false,
            Some("Source truncated below its checkpoint; recovery is unavailable"),
        )?;
        return Ok(());
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut consumed = 0u64;
    let mut oversized = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            store.source_status(&identity, consumed > 0, None)?;
            return Ok(());
        }
        let newline = buffer.iter().position(|b| *b == b'\n');
        let length = newline.map_or(buffer.len(), |i| i + 1);
        if !oversized {
            if line.len() + length <= MAX_LINE {
                line.extend_from_slice(&buffer[..length]);
            } else {
                line.clear();
                oversized = true;
            }
        }
        consumed += length as u64;
        reader.consume(length);
        if newline.is_some() {
            ordinal += 1;
            let record = if oversized {
                Err("Record exceeds the bounded reader limit")
            } else {
                adapter::decode(&line)
            };
            store.line(&identity, offset, offset + consumed, ordinal, record)?;
            offset += consumed;
            consumed = 0;
            oversized = false;
            line.clear();
        }
    }
}
