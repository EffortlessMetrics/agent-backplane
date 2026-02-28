//! Microcrate for portable executable discovery in `PATH`.

use std::path::{Path, PathBuf};

/// Locate an executable by name, similarly to shell `which`.
///
/// If `bin` contains path separators, it is treated as a direct path.
pub fn which(bin: &str) -> Option<PathBuf> {
    let candidate = Path::new(bin);
    if has_path(candidate) {
        return candidate.exists().then(|| candidate.to_path_buf());
    }

    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| resolve_in_dir(&dir, bin))
}

/// Return `true` when an executable can be resolved from PATH or explicit path.
pub fn command_exists(command: &str) -> bool {
    which(command).is_some()
}

fn has_path(candidate: &Path) -> bool {
    candidate.components().count() > 1
}

fn resolve_in_dir(dir: &Path, command: &str) -> Option<PathBuf> {
    let direct = dir.join(command);
    if direct.exists() {
        return Some(direct);
    }

    if !cfg!(windows) {
        return None;
    }

    ["", ".exe", ".cmd", ".bat", ".com"]
        .into_iter()
        .map(|ext| dir.join(format!("{command}{ext}")))
        .find(|candidate| candidate.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_exists_is_consistent_with_which() {
        assert_eq!(
            command_exists("no-such-binary-abp"),
            which("no-such-binary-abp").is_some()
        );
    }
}
