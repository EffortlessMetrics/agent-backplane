//! Small utility crate for PATH/executable discovery.
#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Return true when `command` resolves to an existing executable candidate.
pub fn command_exists(command: &str) -> bool {
    which(command).is_some()
}

/// Resolve a command name to a concrete path.
///
/// Returns `None` when the command cannot be found as a regular file on
/// `PATH` (or at the given relative/absolute path).  Directories are
/// **not** accepted — only regular files pass the check.
pub fn which(command: &str) -> Option<PathBuf> {
    let candidate = Path::new(command);
    if has_path_components(candidate) {
        return path_candidates(candidate)
            .into_iter()
            .find(|path| path.is_file());
    }

    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .flat_map(|dir| path_candidates(&dir.join(command)))
            .find(|path| path.is_file())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn which_rejects_directories() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("fakecmd");
        fs::create_dir(&dir).unwrap();
        // The directory exists but which() must not accept it.
        assert!(which(dir.to_str().unwrap()).is_none());
    }

    #[test]
    fn which_finds_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("realcmd");
        fs::write(&file, b"#!/bin/sh\n").unwrap();
        let result = which(file.to_str().unwrap());
        assert!(result.is_some());
        assert_eq!(result.unwrap(), file);
    }

    #[test]
    fn command_exists_returns_false_for_missing() {
        assert!(!command_exists(
            "/nonexistent/path/that/does/not/exist/binary"
        ));
    }

    #[test]
    #[allow(unsafe_code)]
    fn which_returns_resolved_path_from_path_env() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("mytestcmd");
        fs::write(&file, b"#!/bin/sh\n").unwrap();

        // Temporarily prepend our temp dir to PATH
        let original = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = tmp.path().as_os_str().to_os_string();
        new_path.push(if cfg!(windows) { ";" } else { ":" });
        new_path.push(&original);
        // SAFETY: test-only; single-threaded test manipulating PATH.
        unsafe { std::env::set_var("PATH", &new_path) };

        let result = which("mytestcmd");
        // Restore PATH before asserting
        unsafe { std::env::set_var("PATH", &original) };

        assert!(result.is_some());
        assert_eq!(result.unwrap(), file);
    }
}
