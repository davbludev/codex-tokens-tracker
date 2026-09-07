//! Reads only selected Git administrative path metadata, never Git configuration or content.
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use crate::identity::MAX_PATH_BYTES;

#[derive(Debug, PartialEq, Eq)]
pub struct RepositoryProof {
    pub common_directory: Option<String>,
    pub state: &'static str,
}

fn pointer(path: &Path) -> Option<String> {
    if !path.metadata().ok()?.is_file() {
        return None;
    }
    let file = File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take((MAX_PATH_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_PATH_BYTES {
        return None;
    }
    let value = String::from_utf8(bytes).ok()?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\r', '\n', '\0']) {
        return None;
    }
    Some(value.to_owned())
}

fn directory(base: &Path, value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.len() > MAX_PATH_BYTES {
        return None;
    }
    let path = base.join(value).canonicalize().ok()?;
    path.is_dir().then_some(path)
}

pub fn resolve_repository(root: &Path) -> RepositoryProof {
    let unresolved = || RepositoryProof {
        common_directory: None,
        state: "unresolved",
    };
    if !root.is_absolute()
        || root
            .to_str()
            .is_none_or(|value| value.len() > MAX_PATH_BYTES)
    {
        return unresolved();
    }
    let git = root.join(".git");
    let Ok(metadata) = git.metadata() else {
        return unresolved();
    };
    let (admin, linked) = if metadata.is_dir() {
        let Ok(path) = git.canonicalize() else {
            return unresolved();
        };
        (path, false)
    } else if metadata.is_file() {
        let Some(value) = pointer(&git) else {
            return unresolved();
        };
        let Some(value) = value.strip_prefix("gitdir: ") else {
            return unresolved();
        };
        let Some(path) = directory(root, value) else {
            return unresolved();
        };
        (path, true)
    } else {
        return unresolved();
    };
    let common_path = admin.join("commondir");
    let common = match common_path.try_exists() {
        Ok(true) => {
            let Some(value) = pointer(&common_path) else {
                return unresolved();
            };
            let Some(path) = directory(&admin, &value) else {
                return unresolved();
            };
            path
        }
        Ok(false) if !linked => admin,
        _ => return unresolved(),
    };
    match common.to_str() {
        Some(value) if value.len() <= MAX_PATH_BYTES => RepositoryProof {
            common_directory: Some(value.to_owned()),
            state: "confirmed",
        },
        _ => unresolved(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktrees_share_only_confirmed_common_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("main");
        let linked = temp.path().join("linked");
        let clone = temp.path().join("clone");
        std::fs::create_dir_all(root.join(".git/worktrees/linked")).unwrap();
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::create_dir_all(clone.join(".git")).unwrap();
        std::fs::write(
            linked.join(".git"),
            "gitdir: ../main/.git/worktrees/linked\n",
        )
        .unwrap();
        std::fs::write(root.join(".git/worktrees/linked/commondir"), "../..\n").unwrap();
        assert_eq!(resolve_repository(&root), resolve_repository(&linked));
        assert_eq!(resolve_repository(&root).state, "confirmed");
        assert_ne!(resolve_repository(&root), resolve_repository(&clone));
        for invalid in [
            "",
            "gitdir: missing",
            "gitdir: ../main/.git\nextra",
            "not a pointer",
        ] {
            std::fs::write(linked.join(".git"), invalid).unwrap();
            assert_eq!(resolve_repository(&linked).state, "unresolved");
        }
        std::fs::write(linked.join(".git"), "x".repeat(MAX_PATH_BYTES + 1)).unwrap();
        assert_eq!(resolve_repository(&linked).state, "unresolved");
        std::fs::write(linked.join(".git"), "gitdir: ../main/.git/worktrees/linked").unwrap();
        std::fs::write(root.join(".git/worktrees/linked/commondir"), "missing").unwrap();
        assert_eq!(resolve_repository(&linked).state, "unresolved");
    }
}
