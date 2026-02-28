//! Small utility crate for PATH/executable discovery.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Return true when `command` resolves to an existing executable candidate.
pub fn command_exists(command: &str) -> bool {
    which(command).is_some()
}

/// Resolve a command name to a concrete path.
pub fn which(command: &str) -> Option<PathBuf> {
    let candidate = Path::new(command);
    if has_path_components(candidate) {
        return path_candidates(candidate)
            .into_iter()
            .find(|path| path.exists());
    }

    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .flat_map(|dir| path_candidates(&dir.join(command)))
            .find(|path| path.exists())
    })
}

fn has_path_components(path: &Path) -> bool {
    path.components().count() > 1
}

fn path_candidates(path: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![path.to_path_buf()];

    if cfg!(windows) && path.extension().is_none() {
        let mut ext_paths: Vec<PathBuf> = windows_path_exts()
            .into_iter()
            .map(|ext| {
                let mut os = path.as_os_str().to_os_string();
                os.push(ext);
                PathBuf::from(os)
            })
            .collect();
        candidates.append(&mut ext_paths);
    }

    candidates
}

fn windows_path_exts() -> Vec<OsString> {
    if let Some(path_ext) = std::env::var_os("PATHEXT") {
        let values: Vec<OsString> = path_ext
            .to_string_lossy()
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_ascii_lowercase().into())
            .collect();
        if !values.is_empty() {
            return values;
        }
    }

    [".exe", ".cmd", ".bat", ".com"]
        .into_iter()
        .map(OsString::from)
        .collect()
}
