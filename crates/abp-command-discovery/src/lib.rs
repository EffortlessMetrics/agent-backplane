#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Command discovery helpers shared by sidecar-registration microcrates.

use std::path::Path;

/// Return `true` when `command` resolves to an existing executable path.
///
/// Behavior:
/// - If `command` includes a path separator, check that path directly.
/// - Otherwise, scan entries from `PATH`.
/// - On Windows, also probe common executable extensions.
pub fn command_exists(command: &str) -> bool {
    let candidate = Path::new(command);
    let has_path = candidate.components().count() > 1;

    if has_path {
        return candidate.exists();
    }

    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| path_has_command(&dir, command)))
}

fn path_has_command(dir: &Path, command: &str) -> bool {
    if dir.join(command).exists() {
        return true;
    }

    if !cfg!(windows) {
        return false;
    }

    for ext in ["", ".exe", ".cmd", ".bat", ".com"] {
        if dir.join(format!("{command}{ext}")).exists() {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::command_exists;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn returns_false_for_missing_command() {
        assert!(!command_exists("abp-command-that-should-not-exist-xyz"));
    }

    #[test]
    fn finds_explicit_relative_path() {
        let dir = unique_temp_dir("abp-cmd-discovery");
        let script = dir.join("tool");
        fs::write(&script, "#!/bin/sh\necho ok\n").unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();

        let exists = command_exists("./tool");

        std::env::set_current_dir(cwd).unwrap();
        fs::remove_dir_all(dir).unwrap();
        assert!(exists);
    }
}
