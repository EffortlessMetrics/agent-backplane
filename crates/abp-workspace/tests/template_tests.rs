// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the workspace template system.

use abp_glob::IncludeExcludeGlobs;
use abp_workspace::template::{TemplateRegistry, WorkspaceTemplate};
use std::path::PathBuf;

#[test]
fn create_empty_template() {
    let t = WorkspaceTemplate::new("empty", "An empty template");
    assert_eq!(t.name, "empty");
    assert_eq!(t.description, "An empty template");
    assert_eq!(t.file_count(), 0);
    assert!(t.globs.is_none());
}

#[test]
fn add_files_to_template() {
    let mut t = WorkspaceTemplate::new("demo", "demo template");
    t.add_file("src/main.rs", "fn main() {}");
    t.add_file("README.md", "# Hello");
    assert_eq!(t.file_count(), 2);
    assert!(t.has_file("src/main.rs"));
    assert!(t.has_file("README.md"));
    assert!(!t.has_file("Cargo.toml"));
}

#[test]
fn apply_to_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = WorkspaceTemplate::new("app", "basic app");
    t.add_file("src/main.rs", "fn main() {}");
    t.add_file("Cargo.toml", "[package]\nname = \"app\"");

    let count = t.apply(dir.path()).unwrap();
    assert_eq!(count, 2);
    assert!(dir.path().join("src/main.rs").exists());
    assert!(dir.path().join("Cargo.toml").exists());

    let content = std::fs::read_to_string(dir.path().join("src/main.rs")).unwrap();
    assert_eq!(content, "fn main() {}");
}

#[test]
fn validate_empty_name() {
    let t = WorkspaceTemplate::new("", "desc");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("name is empty")));
}

#[test]
fn validate_empty_description() {
    let t = WorkspaceTemplate::new("ok", "");
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("description is empty")));
}

#[test]
fn validate_absolute_path() {
    let mut t = WorkspaceTemplate::new("bad", "has absolute path");
    // Use a platform-appropriate absolute path.
    let abs = if cfg!(windows) {
        PathBuf::from("C:\\bad\\file.txt")
    } else {
        PathBuf::from("/bad/file.txt")
    };
    t.files.insert(abs, "content".to_string());
    let problems = t.validate();
    assert!(problems.iter().any(|p| p.contains("absolute path")));
}

#[test]
fn validate_valid_template() {
    let mut t = WorkspaceTemplate::new("good", "all fine");
    t.add_file("a.txt", "hello");
    assert!(t.validate().is_empty());
}

#[test]
fn registry_register_and_get() {
    let mut reg = TemplateRegistry::new();
    assert_eq!(reg.count(), 0);

    let t = WorkspaceTemplate::new("alpha", "first");
    reg.register(t);
    assert_eq!(reg.count(), 1);
    assert!(reg.get("alpha").is_some());
    assert!(reg.get("beta").is_none());
}

#[test]
fn registry_list_sorted() {
    let mut reg = TemplateRegistry::new();
    reg.register(WorkspaceTemplate::new("charlie", "c"));
    reg.register(WorkspaceTemplate::new("alpha", "a"));
    reg.register(WorkspaceTemplate::new("bravo", "b"));
    assert_eq!(reg.list(), vec!["alpha", "bravo", "charlie"]);
}

#[test]
fn registry_overwrite() {
    let mut reg = TemplateRegistry::new();
    let mut t1 = WorkspaceTemplate::new("x", "original");
    t1.add_file("a.txt", "old");
    reg.register(t1);

    let mut t2 = WorkspaceTemplate::new("x", "replaced");
    t2.add_file("b.txt", "new");
    reg.register(t2);

    assert_eq!(reg.count(), 1);
    let got = reg.get("x").unwrap();
    assert_eq!(got.description, "replaced");
    assert!(got.has_file("b.txt"));
    assert!(!got.has_file("a.txt"));
}

#[test]
fn serde_roundtrip() {
    let mut t = WorkspaceTemplate::new("serde", "roundtrip test");
    t.add_file("lib.rs", "pub fn hello() {}");
    t.add_file("main.rs", "fn main() {}");

    let json = serde_json::to_string_pretty(&t).unwrap();
    let t2: WorkspaceTemplate = serde_json::from_str(&json).unwrap();

    assert_eq!(t2.name, t.name);
    assert_eq!(t2.description, t.description);
    assert_eq!(t2.file_count(), t.file_count());
    assert!(t2.has_file("lib.rs"));
    assert!(t2.has_file("main.rs"));
    // globs are skipped during serde
    assert!(t2.globs.is_none());
}

#[test]
fn overwrite_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("file.txt"), "old content").unwrap();

    let mut t = WorkspaceTemplate::new("overwrite", "overwrites");
    t.add_file("file.txt", "new content");
    let count = t.apply(dir.path()).unwrap();
    assert_eq!(count, 1);

    let content = std::fs::read_to_string(dir.path().join("file.txt")).unwrap();
    assert_eq!(content, "new content");
}

#[test]
fn template_with_globs() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = WorkspaceTemplate::new("filtered", "uses globs");
    t.add_file("src/lib.rs", "pub mod a;");
    t.add_file("src/generated/out.rs", "// generated");
    t.add_file("README.md", "# Hello");

    let globs =
        IncludeExcludeGlobs::new(&["src/**".to_string()], &["src/generated/**".to_string()])
            .unwrap();
    t.globs = Some(globs);

    let count = t.apply(dir.path()).unwrap();
    // Only src/lib.rs passes (README excluded by include, generated excluded by exclude)
    assert_eq!(count, 1);
    assert!(dir.path().join("src/lib.rs").exists());
    assert!(!dir.path().join("src/generated/out.rs").exists());
    assert!(!dir.path().join("README.md").exists());
}

#[test]
fn large_template() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = WorkspaceTemplate::new("large", "100+ files");
    for i in 0..150 {
        t.add_file(format!("dir/file_{i}.txt"), &format!("content {i}"));
    }
    assert_eq!(t.file_count(), 150);

    let count = t.apply(dir.path()).unwrap();
    assert_eq!(count, 150);

    // Spot-check a few
    assert!(dir.path().join("dir/file_0.txt").exists());
    assert!(dir.path().join("dir/file_149.txt").exists());
    let c = std::fs::read_to_string(dir.path().join("dir/file_42.txt")).unwrap();
    assert_eq!(c, "content 42");
}
