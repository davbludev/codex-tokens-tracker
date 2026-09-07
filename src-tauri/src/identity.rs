//! Decisions over allowlisted identity evidence; no source, database, or filesystem I/O.
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

pub const MAX_LOCATION_CANDIDATES: usize = 128;
pub const MAX_PATH_BYTES: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub struct Location {
    pub path: Option<String>,
    pub state: &'static str,
}

/// Lexical absolute paths only. Never interpret relative evidence against the monitor's cwd.
fn absolute_location(value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.len() > MAX_PATH_BYTES || value.contains('\0') {
        return None;
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return None;
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => (),
            // Resolving .. lexically could cross a filesystem link; do not guess.
            Component::ParentDir => return None,
            other => result.push(other.as_os_str()),
        }
    }
    Some(result)
}

pub fn resolve_location(cwds: &[String], roots: &[String]) -> Location {
    let unresolved = |state| Location { path: None, state };
    if cwds.len() + roots.len() > MAX_LOCATION_CANDIDATES {
        return unresolved("limit");
    }
    let normalize = |values: &[String]| -> Option<BTreeSet<PathBuf>> {
        values
            .iter()
            .map(|value| absolute_location(value))
            .collect()
    };
    let (Some(cwds), Some(roots)) = (normalize(cwds), normalize(roots)) else {
        return unresolved("ambiguous");
    };
    let selected = if roots.is_empty() {
        if cwds.len() > 1 {
            return unresolved("ambiguous");
        }
        cwds.iter().next()
    } else if cwds.is_empty() {
        if roots.len() > 1 {
            return unresolved("ambiguous");
        }
        roots.iter().next()
    } else {
        let mut selected = None;
        for cwd in &cwds {
            let mut matches = roots.iter().filter(|root| cwd.starts_with(root));
            let Some(root) = matches.next() else {
                return unresolved("ambiguous");
            };
            if matches.next().is_some() || selected.is_some_and(|previous| previous != root) {
                return unresolved("ambiguous");
            }
            selected = Some(root);
        }
        selected
    };
    match selected.and_then(|path| path.to_str()) {
        Some(path) => Location {
            path: Some(path.to_owned()),
            state: "available",
        },
        None => unresolved("unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_requires_one_consistent_exact_bucket() {
        let base = std::env::temp_dir();
        let a = base.join("identity-a").to_str().unwrap().to_owned();
        let b = base.join("identity-b").to_str().unwrap().to_owned();
        let child = Path::new(&a).join("child").to_str().unwrap().to_owned();
        assert_eq!(resolve_location(&[a.clone()], &[]).path, Some(a.clone()));
        assert_eq!(
            resolve_location(&[child.clone()], &[a.clone(), b.clone()]).path,
            Some(a.clone())
        );
        assert_eq!(
            resolve_location(&[child.clone()], &[a.clone(), child]).state,
            "ambiguous"
        );
        assert_eq!(
            resolve_location(&[a.clone(), b.clone()], &[]).state,
            "ambiguous"
        );
        assert_eq!(
            resolve_location(&[b.clone()], &[a.clone()]).state,
            "ambiguous"
        );
        assert_eq!(resolve_location(&[], &[a.clone(), b]).state, "ambiguous");
        assert_eq!(resolve_location(&[], &[a.clone()]).path, Some(a));
        assert_eq!(resolve_location(&[], &[]).state, "unavailable");
        assert_eq!(
            resolve_location(&["relative/path".into()], &[]).state,
            "ambiguous"
        );
        assert_eq!(
            resolve_location(&vec!["a".into(); MAX_LOCATION_CANDIDATES + 1], &[]).state,
            "limit"
        );
    }
}
