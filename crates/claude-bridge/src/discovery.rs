use std::path::{Path, PathBuf};

use crate::BridgeError;

/// Default node command.
pub const DEFAULT_NODE_COMMAND: &str = "node";

/// Relative path to the Claude host script.
pub const HOST_SCRIPT_RELATIVE: &str = "hosts/claude/host.js";

/// Environment variable for overriding the host script path.
pub const HOST_SCRIPT_ENV: &str = "ABP_CLAUDE_HOST_SCRIPT";

/// Resolve the node command to use.
pub fn resolve_node(override_command: Option<&str>) -> Result<String, BridgeError> {
    if let Some(cmd) = override_command {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            if command_exists(cmd) {
                return Ok(cmd.to_string());
            }
            return Err(BridgeError::NodeNotFound(format!(
                "explicit node command '{cmd}' not found"
            )));
        }
    }

    if command_exists(DEFAULT_NODE_COMMAND) {
        return Ok(DEFAULT_NODE_COMMAND.to_string());
    }

    Err(BridgeError::NodeNotFound(
        "node not found in PATH".to_string(),
    ))
}

/// Resolve the host script path. Search order:
/// 1. Explicit path from config
/// 2. ABP_CLAUDE_HOST_SCRIPT environment variable
/// 3. hosts/claude/host.js relative to CWD
/// 4. ~/.agent-backplane/hosts/claude/host.js
pub fn resolve_host_script(explicit: Option<&Path>) -> Result<PathBuf, BridgeError> {
    // 1. Explicit path
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        return Err(BridgeError::HostScriptNotFound(format!(
            "explicit host script not found: {}",
            path.display()
        )));
    }

    // 2. Environment variable
    if let Ok(path) = std::env::var(HOST_SCRIPT_ENV) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }

    // 3. CWD-relative
    let cwd_relative = PathBuf::from(HOST_SCRIPT_RELATIVE);
    if cwd_relative.is_file() {
        return Ok(cwd_relative);
    }

    // 4. Home directory
    if let Some(home) = home_dir() {
        let home_path = home.join(".agent-backplane").join(HOST_SCRIPT_RELATIVE);
        if home_path.is_file() {
            return Ok(home_path);
        }
    }

    Err(BridgeError::HostScriptNotFound(
        "could not find hosts/claude/host.js in any search path".to_string(),
    ))
}

fn command_exists(command: &str) -> bool {
    let candidate = Path::new(command);
    let has_path = candidate.components().count() > 1;

    if has_path {
        return candidate.exists();
    }

    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| path_has_command(&dir, command))
    })
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

fn home_dir() -> Option<PathBuf> {
    // Try HOME first (Unix + some Windows), then USERPROFILE (Windows)
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
