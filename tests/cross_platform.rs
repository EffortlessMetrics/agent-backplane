// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-platform compatibility tests.
//!
//! Validates path handling, line endings, temp directory usage, process
//! spawning, Unicode paths, case sensitivity, long paths, concurrent
//! file access, and path normalisation across Windows and Unix.

use std::fs;
use std::path::{Path, PathBuf};

use abp_cli::config::{load_config, BackendConfig};
use abp_glob::{IncludeExcludeGlobs, MatchDecision};
use abp_protocol::{Envelope, JsonlCodec};

fn patterns(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

// -----------------------------------------------------------------------
// 1. Path separators: forward / backward slash handling in globs
// -----------------------------------------------------------------------

#[test]
fn glob_matches_forward_slash_paths() {
    let globs = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &[]).unwrap();
    assert_eq!(globs.decide_str("src/lib.rs"), MatchDecision::Allowed);
    assert_eq!(globs.decide_str("src/a/b/c.rs"), MatchDecision::Allowed);
}

#[test]
fn glob_matches_native_path_separators() {
    let globs = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &[]).unwrap();
    // Build a path using the OS-native separator via PathBuf.
    let native = PathBuf::from("src").join("lib.rs");
    assert_eq!(globs.decide_path(&native), MatchDecision::Allowed);
}

#[test]
fn glob_exclude_with_native_paths() {
    let globs =
        IncludeExcludeGlobs::new(&patterns(&["**"]), &patterns(&["target/**"])).unwrap();
    let native = PathBuf::from("target").join("debug").join("build");
    assert_eq!(globs.decide_path(&native), MatchDecision::DeniedByExclude);
    let ok = PathBuf::from("src").join("main.rs");
    assert_eq!(globs.decide_path(&ok), MatchDecision::Allowed);
}

// -----------------------------------------------------------------------
// 2. File permissions: tests that work on both Windows and Unix
// -----------------------------------------------------------------------

#[test]
fn read_only_file_is_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("readonly.txt");
    fs::write(&file, "hello").unwrap();

    // Make read-only in a cross-platform way.
    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file, perms).unwrap();

    // Should still be readable.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hello");

    // Restore writability so tempdir cleanup works on Windows.
    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(false);
    fs::set_permissions(&file, perms).unwrap();
}

#[cfg(unix)]
#[test]
fn unix_executable_permission_preserved_in_staged_workspace() {
    use std::os::unix::fs::PermissionsExt;

    let tmp_src = tempfile::tempdir().unwrap();
    let script = tmp_src.path().join("run.sh");
    fs::write(&script, "#!/bin/sh\necho ok\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(tmp_src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged = ws.path().join("run.sh");
    let mode = fs::metadata(&staged).unwrap().permissions().mode();
    assert_ne!(mode & 0o111, 0, "execute bit should be set");
}

// -----------------------------------------------------------------------
// 3. Line endings: CRLF vs LF in JSONL parsing
// -----------------------------------------------------------------------

#[test]
fn jsonl_decode_with_lf() {
    let line = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}";
    let env = JsonlCodec::decode(line).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn jsonl_decode_with_crlf_trimmed() {
    // Sidecars on Windows may emit \r\n. The host trims lines before decoding.
    let line = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\r\n";
    let trimmed = line.trim();
    let env = JsonlCodec::decode(trimmed).unwrap();
    assert!(matches!(env, Envelope::Fatal { .. }));
}

#[test]
fn jsonl_decode_stream_handles_mixed_line_endings() {
    use std::io::BufReader;

    // Mix LF and CRLF lines.
    let input = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"a\"}\n\
                 {\"t\":\"fatal\",\"ref_id\":null,\"error\":\"b\"}\r\n\
                 {\"t\":\"fatal\",\"ref_id\":null,\"error\":\"c\"}\n";
    let reader = BufReader::new(input.as_bytes());
    let results: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(results.len(), 3);
}

// -----------------------------------------------------------------------
// 4. Temp directory: cross-platform temp dir usage
// -----------------------------------------------------------------------

#[test]
fn tempdir_is_writable_and_removable() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("test.txt");
    fs::write(&file, "data").unwrap();
    assert!(file.exists());

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "data");

    // Drop removes the directory.
    let path = tmp.path().to_path_buf();
    drop(tmp);
    assert!(!path.exists());
}

#[test]
fn workspace_stager_uses_temp_directory() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("hello.txt"), "world").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    assert!(ws.path().join("hello.txt").exists());
    // Workspace temp dir lives under the system temp area.
    let temp_root = std::env::temp_dir();
    assert!(
        ws.path().starts_with(&temp_root),
        "staged workspace {} should be under temp dir {}",
        ws.path().display(),
        temp_root.display()
    );
}

// -----------------------------------------------------------------------
// 5. Process spawning: shell command differences
// -----------------------------------------------------------------------

#[test]
fn spawn_echo_command_cross_platform() {
    let output = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "echo hello"])
            .output()
            .unwrap()
    } else {
        std::process::Command::new("sh")
            .args(["-c", "echo hello"])
            .output()
            .unwrap()
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().contains("hello"),
        "expected 'hello' in output: {stdout}"
    );
}

#[test]
fn sidecar_spec_command_varies_by_platform() {
    use abp_host::SidecarSpec;

    // On Windows the shell wrapper is `cmd /C`, on Unix `sh -c`.
    let spec = if cfg!(target_os = "windows") {
        SidecarSpec {
            command: "cmd".into(),
            args: vec!["/C".into(), "echo hi".into()],
            env: Default::default(),
            cwd: None,
        }
    } else {
        SidecarSpec {
            command: "sh".into(),
            args: vec!["-c".into(), "echo hi".into()],
            env: Default::default(),
            cwd: None,
        }
    };

    // Just verify the struct is well-formed.
    assert!(!spec.command.is_empty());
    assert!(!spec.args.is_empty());
}

// -----------------------------------------------------------------------
// 6. Unicode paths: non-ASCII directory names
// -----------------------------------------------------------------------

#[test]
fn unicode_directory_creation_and_file_io() {
    let tmp = tempfile::tempdir().unwrap();
    let uni_dir = tmp.path().join("données").join("日本語");
    fs::create_dir_all(&uni_dir).unwrap();
    let file = uni_dir.join("файл.txt");
    fs::write(&file, "содержание").unwrap();
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "содержание");
}

#[test]
fn glob_matches_unicode_paths() {
    let globs = IncludeExcludeGlobs::new(&patterns(&["données/**"]), &[]).unwrap();
    let p = PathBuf::from("données").join("résultat.txt");
    assert_eq!(globs.decide_path(&p), MatchDecision::Allowed);
}

#[test]
fn workspace_stager_with_unicode_content() {
    let src = tempfile::tempdir().unwrap();
    let sub = src.path().join("ñoño");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("café.txt"), "olé").unwrap();

    let ws = abp_workspace::WorkspaceStager::new()
        .source_root(src.path())
        .with_git_init(false)
        .stage()
        .unwrap();

    let staged_file = ws.path().join("ñoño").join("café.txt");
    assert!(staged_file.exists(), "unicode-named file should be staged");
    assert_eq!(fs::read_to_string(&staged_file).unwrap(), "olé");
}

// -----------------------------------------------------------------------
// 7. Case sensitivity: filename case handling per platform
// -----------------------------------------------------------------------

#[test]
fn case_sensitivity_of_file_system() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("File.txt"), "upper").unwrap();

    // On case-insensitive FS (Windows, macOS default), opening "file.txt"
    // returns the same file. On case-sensitive FS (Linux), it won't exist.
    let lower = tmp.path().join("file.txt");
    if lower.exists() {
        // Case-insensitive: reads the same content.
        let content = fs::read_to_string(&lower).unwrap();
        assert_eq!(content, "upper");
    } else {
        // Case-sensitive: file truly absent.
        assert!(fs::read_to_string(&lower).is_err());
    }
}

#[test]
fn glob_case_sensitivity_follows_platform() {
    // globset is always case-sensitive regardless of platform.
    // On a case-insensitive filesystem (Windows), the FS itself will resolve
    // names, but glob pattern matching remains literal.
    let globs = IncludeExcludeGlobs::new(&patterns(&["*.TXT"]), &[]).unwrap();
    assert_eq!(
        globs.decide_str("readme.TXT"),
        MatchDecision::Allowed,
        "exact case should always match"
    );
    // Lower-case extension never matches the upper-case-only pattern.
    assert_eq!(
        globs.decide_str("readme.txt"),
        MatchDecision::DeniedByMissingInclude,
        "globset is case-sensitive"
    );
}

// -----------------------------------------------------------------------
// 8. Long paths: handle > 260 char paths (Windows limitation)
// -----------------------------------------------------------------------

#[test]
fn long_path_file_creation() {
    let tmp = tempfile::tempdir().unwrap();

    // Build a deeply nested path that exceeds 260 chars.
    let mut long = tmp.path().to_path_buf();
    for _ in 0..30 {
        long = long.join("abcdefghij");
    }

    // On Windows, long paths may need the \\?\ prefix or registry setting.
    // Use the extended-length path prefix on Windows.
    let create_path = if cfg!(target_os = "windows") {
        let canonical = tmp.path().canonicalize().unwrap();
        let mut extended = canonical;
        for _ in 0..30 {
            extended = extended.join("abcdefghij");
        }
        // Prepend \\?\ for extended-length support.
        let s = format!("\\\\?\\{}", extended.display());
        PathBuf::from(s)
    } else {
        long.clone()
    };

    match fs::create_dir_all(&create_path) {
        Ok(_) => {
            let file = create_path.join("deep.txt");
            fs::write(&file, "deep content").unwrap();
            assert_eq!(fs::read_to_string(&file).unwrap(), "deep content");
        }
        Err(e) => {
            // Some Windows configurations may still reject long paths.
            eprintln!("long path creation failed (expected on some systems): {e}");
        }
    }
}

// -----------------------------------------------------------------------
// 9. Concurrent file access: multiple readers/writers
// -----------------------------------------------------------------------

#[tokio::test]
async fn concurrent_file_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("shared.txt");
    fs::write(&file_path, "shared data").unwrap();

    let mut handles = Vec::new();
    for _ in 0..10 {
        let p = file_path.clone();
        handles.push(tokio::spawn(async move {
            tokio::fs::read_to_string(&p).await.unwrap()
        }));
    }

    for h in handles {
        let content = h.await.unwrap();
        assert_eq!(content, "shared data");
    }
}

#[tokio::test]
async fn concurrent_file_writes_to_separate_files() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().to_path_buf();

    let mut handles = Vec::new();
    for i in 0..10u32 {
        let dir = base.clone();
        handles.push(tokio::spawn(async move {
            let path = dir.join(format!("file_{i}.txt"));
            tokio::fs::write(&path, format!("content-{i}")).await.unwrap();
            tokio::fs::read_to_string(&path).await.unwrap()
        }));
    }

    for (i, h) in handles.into_iter().enumerate() {
        let content = h.await.unwrap();
        assert_eq!(content, format!("content-{i}"));
    }
}

// -----------------------------------------------------------------------
// 10. Path normalisation: relative paths with .. and .
// -----------------------------------------------------------------------

#[test]
fn path_normalisation_with_dot_segments() {
    let base = PathBuf::from("a").join("b").join("..").join("c");
    // std PathBuf does NOT normalise `..`; it preserves them literally.
    assert!(base.to_string_lossy().contains(".."));

    // After canonicalize on real paths, `..` is resolved.
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("a").join("b");
    fs::create_dir_all(&real).unwrap();
    let with_dots = real.join("..").join("c");
    fs::create_dir_all(&with_dots).unwrap();
    let canonical = with_dots.canonicalize().unwrap();
    assert!(
        !canonical.to_string_lossy().contains(".."),
        "canonical path should not contain '..': {}",
        canonical.display()
    );
    // The resolved path should end with "a/c" or "a\\c".
    let ends = canonical.ends_with(Path::new("a").join("c"));
    assert!(ends, "expected path ending with a/c: {}", canonical.display());
}

#[test]
fn glob_with_normalised_and_raw_paths() {
    let globs = IncludeExcludeGlobs::new(&patterns(&["src/**"]), &[]).unwrap();
    // A path with `.` segments still matches because globset treats them as
    // literal path components and `*` crosses separators by default.
    assert_eq!(globs.decide_str("src/./lib.rs"), MatchDecision::Allowed);
}

// -----------------------------------------------------------------------
// Bonus: config loading cross-platform
// -----------------------------------------------------------------------

#[test]
fn config_load_from_cross_platform_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("backplane.toml");
    let toml_content = "\
default_backend = \"mock\"\n\
\n\
[backends.mock]\n\
type = \"mock\"\n\
\n\
[backends.sc]\n\
type = \"sidecar\"\n\
command = \"node\"\n\
args = [\"host.js\"]\n\
";
    fs::write(&cfg_path, toml_content).unwrap();
    let config = load_config(Some(&cfg_path)).unwrap();
    assert_eq!(config.default_backend.as_deref(), Some("mock"));
    assert!(config.backends.contains_key("sc"));
    assert!(matches!(
        config.backends.get("mock"),
        Some(BackendConfig::Mock {})
    ));
}

#[test]
fn config_path_with_spaces_and_unicode() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("my configs").join("中文");
    fs::create_dir_all(&dir).unwrap();
    let cfg_path = dir.join("backplane.toml");
    fs::write(&cfg_path, "default_backend = \"test\"\n").unwrap();
    let config = load_config(Some(&cfg_path)).unwrap();
    assert_eq!(config.default_backend.as_deref(), Some("test"));
}
