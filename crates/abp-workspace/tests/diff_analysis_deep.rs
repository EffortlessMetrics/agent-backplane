#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep tests for structured diff analysis: `DiffAnalysis`, `ChangeClassifier`,
//! `DiffReport`, and supporting types.

use abp_workspace::diff::{
    identify_file_type, ChangeClassifier, DiffAnalysis, DiffChangeKind, DiffLineKind, DiffReport,
    FileBreakdown, FileCategory, FileStats, FileType, RiskLevel,
};

// ===========================================================================
// DiffAnalysis::parse — basic cases
// ===========================================================================

// 1
#[test]
fn parse_empty_string() {
    let a = DiffAnalysis::parse("");
    assert!(a.is_empty());
    assert_eq!(a.file_count(), 0);
    assert_eq!(a.total_additions, 0);
    assert_eq!(a.total_deletions, 0);
    assert_eq!(a.binary_file_count, 0);
}

// 2
#[test]
fn parse_whitespace_only() {
    let a = DiffAnalysis::parse("   \n\n  \n");
    assert!(a.is_empty());
}

// 3
#[test]
fn parse_single_added_file() {
    let raw = "\
diff --git a/hello.txt b/hello.txt
new file mode 100644
index 0000000..ce01362
--- /dev/null
+++ b/hello.txt
@@ -0,0 +1,2 @@
+hello
+world
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert_eq!(a.files[0].change_kind, DiffChangeKind::Added);
    assert_eq!(a.files[0].path, "hello.txt");
    assert_eq!(a.files[0].additions, 2);
    assert_eq!(a.files[0].deletions, 0);
    assert!(!a.files[0].is_binary);
    assert_eq!(a.total_additions, 2);
    assert_eq!(a.total_deletions, 0);
}

// 4
#[test]
fn parse_single_modified_file() {
    let raw = "\
diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!(\"old\");
+    println!(\"new\");
+    println!(\"extra\");
 }
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert_eq!(a.files[0].change_kind, DiffChangeKind::Modified);
    assert_eq!(a.files[0].path, "src/main.rs");
    assert_eq!(a.files[0].additions, 2);
    assert_eq!(a.files[0].deletions, 1);
    assert_eq!(a.files[0].file_type, FileType::Rust);
}

// 5
#[test]
fn parse_single_deleted_file() {
    let raw = "\
diff --git a/old.txt b/old.txt
deleted file mode 100644
index abc1234..0000000
--- a/old.txt
+++ /dev/null
@@ -1,3 +0,0 @@
-line1
-line2
-line3
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert_eq!(a.files[0].change_kind, DiffChangeKind::Deleted);
    assert_eq!(a.files[0].deletions, 3);
    assert_eq!(a.files[0].additions, 0);
    assert_eq!(a.total_deletions, 3);
}

// 6
#[test]
fn parse_renamed_file() {
    let raw = "\
diff --git a/old_name.rs b/new_name.rs
similarity index 100%
rename from old_name.rs
rename to new_name.rs
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert_eq!(a.files[0].change_kind, DiffChangeKind::Renamed);
    assert_eq!(a.files[0].path, "new_name.rs");
    assert_eq!(a.files[0].renamed_from, Some("old_name.rs".to_string()));
    assert_eq!(a.files[0].additions, 0);
    assert_eq!(a.files[0].deletions, 0);
}

// 7
#[test]
fn parse_binary_new_file() {
    let raw = "\
diff --git a/image.png b/image.png
new file mode 100644
index 0000000..abc1234
Binary files /dev/null and b/image.png differ
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert!(a.files[0].is_binary);
    assert_eq!(a.files[0].change_kind, DiffChangeKind::Added);
    assert_eq!(a.files[0].file_type, FileType::Binary);
    assert_eq!(a.binary_file_count, 1);
    assert_eq!(a.files[0].additions, 0);
}

// 8
#[test]
fn parse_binary_modified_file() {
    let raw = "\
diff --git a/data.bin b/data.bin
index abc1234..def5678 100644
Binary files a/data.bin and b/data.bin differ
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert!(a.files[0].is_binary);
    assert_eq!(a.files[0].change_kind, DiffChangeKind::Modified);
    assert_eq!(a.binary_file_count, 1);
}

// 9
#[test]
fn parse_multiple_files() {
    let raw = "\
diff --git a/a.txt b/a.txt
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/a.txt
@@ -0,0 +1 @@
+alpha
diff --git a/b.py b/b.py
index abc1234..def5678 100644
--- a/b.py
+++ b/b.py
@@ -1 +1 @@
-old
+new
diff --git a/c.rs b/c.rs
deleted file mode 100644
index abc1234..0000000
--- a/c.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn foo() {}
-fn bar() {}
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 3);
    assert_eq!(a.total_additions, 2); // 1 (a.txt) + 1 (b.py)
    assert_eq!(a.total_deletions, 3); // 1 (b.py) + 2 (c.rs)

    let added = a.files_by_kind(DiffChangeKind::Added);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].path, "a.txt");

    let modified = a.files_by_kind(DiffChangeKind::Modified);
    assert_eq!(modified.len(), 1);
    assert_eq!(modified[0].path, "b.py");

    let deleted = a.files_by_kind(DiffChangeKind::Deleted);
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].path, "c.rs");
}

// 10
#[test]
fn parse_permission_change_only() {
    let raw = "\
diff --git a/script.sh b/script.sh
old mode 100644
new mode 100755
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert_eq!(a.files[0].change_kind, DiffChangeKind::Modified);
    assert_eq!(a.files[0].old_mode, Some("100644".to_string()));
    assert_eq!(a.files[0].new_mode, Some("100755".to_string()));
    assert_eq!(a.files[0].additions, 0);
    assert_eq!(a.files[0].deletions, 0);
}

// 11
#[test]
fn parse_no_newline_at_end_of_file() {
    let raw = "\
diff --git a/no_nl.txt b/no_nl.txt
index abc1234..def5678 100644
--- a/no_nl.txt
+++ b/no_nl.txt
@@ -1 +1 @@
-old
\\ No newline at end of file
+new
\\ No newline at end of file
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert_eq!(a.files[0].additions, 1);
    assert_eq!(a.files[0].deletions, 1);

    let hunk = &a.files[0].hunks[0];
    let no_nl_count = hunk
        .lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::NoNewlineMarker)
        .count();
    assert_eq!(no_nl_count, 2);
}

// 12
#[test]
fn parse_multiple_hunks_in_one_file() {
    let raw = "\
diff --git a/multi.txt b/multi.txt
index abc1234..def5678 100644
--- a/multi.txt
+++ b/multi.txt
@@ -1,3 +1,3 @@
 line1
-old_line2
+new_line2
 line3
@@ -10,3 +10,4 @@
 line10
 line11
+inserted
 line12
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.files[0].hunks.len(), 2);
    assert_eq!(a.files[0].additions, 2);
    assert_eq!(a.files[0].deletions, 1);

    assert_eq!(a.files[0].hunks[0].old_start, 1);
    assert_eq!(a.files[0].hunks[0].old_count, 3);
    assert_eq!(a.files[0].hunks[1].new_start, 10);
    assert_eq!(a.files[0].hunks[1].new_count, 4);
}

// 13
#[test]
fn parse_context_lines() {
    let raw = "\
diff --git a/ctx.txt b/ctx.txt
index abc1234..def5678 100644
--- a/ctx.txt
+++ b/ctx.txt
@@ -1,5 +1,5 @@
 alpha
 beta
-gamma
+GAMMA
 delta
 epsilon
";
    let a = DiffAnalysis::parse(raw);
    let hunk = &a.files[0].hunks[0];
    let ctx_count = hunk
        .lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Context)
        .count();
    assert_eq!(ctx_count, 4);
    assert_eq!(hunk.lines[0].kind, DiffLineKind::Context);
    assert_eq!(hunk.lines[0].content, "alpha");
}

// 14
#[test]
fn parse_hunk_header_with_function_name() {
    let raw = "\
diff --git a/func.rs b/func.rs
index abc1234..def5678 100644
--- a/func.rs
+++ b/func.rs
@@ -10,6 +10,7 @@ fn existing_function() {
     let x = 1;
+    let y = 2;
     let z = 3;
";
    let a = DiffAnalysis::parse(raw);
    let hunk = &a.files[0].hunks[0];
    assert!(hunk.header.contains("fn existing_function()"));
    assert_eq!(hunk.old_start, 10);
    assert_eq!(hunk.old_count, 6);
    assert_eq!(hunk.new_start, 10);
    assert_eq!(hunk.new_count, 7);
}

// 15
#[test]
fn parse_hunk_header_single_line_range() {
    let raw = "\
diff --git a/one.txt b/one.txt
index abc1234..def5678 100644
--- a/one.txt
+++ b/one.txt
@@ -1 +1 @@
-old
+new
";
    let a = DiffAnalysis::parse(raw);
    let hunk = &a.files[0].hunks[0];
    assert_eq!(hunk.old_start, 1);
    assert_eq!(hunk.old_count, 1);
    assert_eq!(hunk.new_start, 1);
    assert_eq!(hunk.new_count, 1);
}

// 16
#[test]
fn parse_added_file_with_content_lines() {
    let raw = "\
diff --git a/new.py b/new.py
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/new.py
@@ -0,0 +1,4 @@
+import os
+import sys
+
+print('hello')
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.files[0].additions, 4);
    assert_eq!(a.files[0].file_type, FileType::Python);
    assert_eq!(a.files[0].new_mode, Some("100644".to_string()));
}

// 17
#[test]
fn parse_deleted_file_with_content_lines() {
    let raw = "\
diff --git a/removed.js b/removed.js
deleted file mode 100644
index abc1234..0000000
--- a/removed.js
+++ /dev/null
@@ -1,3 +0,0 @@
-const x = 1;
-const y = 2;
-module.exports = { x, y };
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.files[0].deletions, 3);
    assert_eq!(a.files[0].file_type, FileType::JavaScript);
    assert_eq!(a.files[0].old_mode, Some("100644".to_string()));
}

// 18
#[test]
fn parse_mixed_add_modify_delete() {
    let raw = "\
diff --git a/added.ts b/added.ts
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/added.ts
@@ -0,0 +1 @@
+export const x = 1;
diff --git a/modified.go b/modified.go
index abc1234..def5678 100644
--- a/modified.go
+++ b/modified.go
@@ -1,2 +1,2 @@
-func old() {}
+func updated() {}
 func keep() {}
diff --git a/deleted.c b/deleted.c
deleted file mode 100644
index abc1234..0000000
--- a/deleted.c
+++ /dev/null
@@ -1 +0,0 @@
-int main() { return 0; }
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 3);
    assert_eq!(a.total_additions, 2);
    assert_eq!(a.total_deletions, 2);
    assert_eq!(a.files_by_kind(DiffChangeKind::Added).len(), 1);
    assert_eq!(a.files_by_kind(DiffChangeKind::Modified).len(), 1);
    assert_eq!(a.files_by_kind(DiffChangeKind::Deleted).len(), 1);
}

// 19
#[test]
fn parse_renamed_file_with_content_changes() {
    let raw = "\
diff --git a/old.rs b/new.rs
similarity index 80%
rename from old.rs
rename to new.rs
index abc1234..def5678 100644
--- a/old.rs
+++ b/new.rs
@@ -1,2 +1,3 @@
 fn keep() {}
-fn old_fn() {}
+fn new_fn() {}
+fn extra() {}
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.files[0].change_kind, DiffChangeKind::Renamed);
    assert_eq!(a.files[0].path, "new.rs");
    assert_eq!(a.files[0].renamed_from, Some("old.rs".to_string()));
    assert_eq!(a.files[0].additions, 2);
    assert_eq!(a.files[0].deletions, 1);
}

// 20
#[test]
fn parse_large_diff_many_files() {
    let mut raw = String::new();
    for i in 0..25 {
        raw.push_str(&format!(
            "\
diff --git a/file_{i}.txt b/file_{i}.txt
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/file_{i}.txt
@@ -0,0 +1 @@
+content {i}
"
        ));
    }
    let a = DiffAnalysis::parse(&raw);
    assert_eq!(a.file_count(), 25);
    assert_eq!(a.total_additions, 25);
    assert_eq!(a.binary_file_count, 0);
}

// ===========================================================================
// DiffAnalysis methods
// ===========================================================================

// 21
#[test]
fn analysis_is_empty_on_default() {
    let a = DiffAnalysis::default();
    assert!(a.is_empty());
}

// 22
#[test]
fn analysis_is_empty_false_when_has_files() {
    let raw = "\
diff --git a/f.txt b/f.txt
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/f.txt
@@ -0,0 +1 @@
+x
";
    let a = DiffAnalysis::parse(raw);
    assert!(!a.is_empty());
}

// 23
#[test]
fn analysis_file_stats() {
    let raw = "\
diff --git a/lib.rs b/lib.rs
index abc1234..def5678 100644
--- a/lib.rs
+++ b/lib.rs
@@ -1,2 +1,3 @@
 fn a() {}
-fn b() {}
+fn b_new() {}
+fn c() {}
";
    let a = DiffAnalysis::parse(raw);
    let stats = a.file_stats();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].path, "lib.rs");
    assert_eq!(stats[0].additions, 2);
    assert_eq!(stats[0].deletions, 1);
    assert!(!stats[0].is_binary);
    assert_eq!(stats[0].file_type, FileType::Rust);
    assert_eq!(stats[0].change_kind, DiffChangeKind::Modified);
}

// 24
#[test]
fn analysis_files_by_kind_returns_empty_for_no_match() {
    let raw = "\
diff --git a/f.txt b/f.txt
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/f.txt
@@ -0,0 +1 @@
+x
";
    let a = DiffAnalysis::parse(raw);
    assert!(a.files_by_kind(DiffChangeKind::Deleted).is_empty());
    assert!(a.files_by_kind(DiffChangeKind::Renamed).is_empty());
}

// 25
#[test]
fn analysis_serde_roundtrip() {
    let raw = "\
diff --git a/f.txt b/f.txt
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/f.txt
@@ -0,0 +1 @@
+hello
";
    let a = DiffAnalysis::parse(raw);
    let json = serde_json::to_string(&a).unwrap();
    let rt: DiffAnalysis = serde_json::from_str(&json).unwrap();
    assert_eq!(a, rt);
}

// ===========================================================================
// identify_file_type
// ===========================================================================

// 26
#[test]
fn file_type_rust() {
    assert_eq!(identify_file_type("src/lib.rs"), FileType::Rust);
}

// 27
#[test]
fn file_type_javascript() {
    assert_eq!(identify_file_type("index.js"), FileType::JavaScript);
    assert_eq!(identify_file_type("module.mjs"), FileType::JavaScript);
    assert_eq!(identify_file_type("common.cjs"), FileType::JavaScript);
}

// 28
#[test]
fn file_type_typescript() {
    assert_eq!(identify_file_type("app.ts"), FileType::TypeScript);
    assert_eq!(identify_file_type("component.tsx"), FileType::TypeScript);
}

// 29
#[test]
fn file_type_python() {
    assert_eq!(identify_file_type("main.py"), FileType::Python);
    assert_eq!(identify_file_type("types.pyi"), FileType::Python);
}

// 30
#[test]
fn file_type_go() {
    assert_eq!(identify_file_type("main.go"), FileType::Go);
}

// 31
#[test]
fn file_type_html() {
    assert_eq!(identify_file_type("index.html"), FileType::Html);
    assert_eq!(identify_file_type("page.htm"), FileType::Html);
}

// 32
#[test]
fn file_type_css() {
    assert_eq!(identify_file_type("style.css"), FileType::Css);
    assert_eq!(identify_file_type("theme.scss"), FileType::Css);
}

// 33
#[test]
fn file_type_json() {
    assert_eq!(identify_file_type("data.json"), FileType::Json);
}

// 34
#[test]
fn file_type_yaml() {
    assert_eq!(identify_file_type("config.yaml"), FileType::Yaml);
    assert_eq!(identify_file_type("config.yml"), FileType::Yaml);
}

// 35
#[test]
fn file_type_toml() {
    assert_eq!(identify_file_type("Cargo.toml"), FileType::Toml);
}

// 36
#[test]
fn file_type_markdown() {
    assert_eq!(identify_file_type("README.md"), FileType::Markdown);
}

// 37
#[test]
fn file_type_shell() {
    assert_eq!(identify_file_type("build.sh"), FileType::Shell);
    assert_eq!(identify_file_type("script.ps1"), FileType::Shell);
}

// 38
#[test]
fn file_type_binary_extensions() {
    assert_eq!(identify_file_type("logo.png"), FileType::Binary);
    assert_eq!(identify_file_type("photo.jpg"), FileType::Binary);
    assert_eq!(identify_file_type("archive.zip"), FileType::Binary);
    assert_eq!(identify_file_type("font.woff2"), FileType::Binary);
    assert_eq!(identify_file_type("app.exe"), FileType::Binary);
    assert_eq!(identify_file_type("lib.dll"), FileType::Binary);
}

// 39
#[test]
fn file_type_unknown_extension() {
    assert_eq!(identify_file_type("data.xyz"), FileType::Other);
    assert_eq!(identify_file_type("file.custom"), FileType::Other);
}

// 40
#[test]
fn file_type_no_extension() {
    assert_eq!(identify_file_type("Makefile"), FileType::Other);
    assert_eq!(identify_file_type("Dockerfile"), FileType::Other);
}

// 41
#[test]
fn file_type_java() {
    assert_eq!(identify_file_type("Main.java"), FileType::Java);
}

// 42
#[test]
fn file_type_csharp() {
    assert_eq!(identify_file_type("Program.cs"), FileType::CSharp);
}

// 43
#[test]
fn file_type_cpp() {
    assert_eq!(identify_file_type("main.cpp"), FileType::Cpp);
    assert_eq!(identify_file_type("header.hpp"), FileType::Cpp);
}

// 44
#[test]
fn file_type_c() {
    assert_eq!(identify_file_type("main.c"), FileType::C);
    assert_eq!(identify_file_type("header.h"), FileType::C);
}

// 45
#[test]
fn file_type_sql() {
    assert_eq!(identify_file_type("schema.sql"), FileType::Sql);
}

// 46
#[test]
fn file_type_xml() {
    assert_eq!(identify_file_type("pom.xml"), FileType::Xml);
}

// ===========================================================================
// ChangeClassifier — classify_path
// ===========================================================================

// 47
#[test]
fn classify_source_code_rust() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("src/lib.rs"), FileCategory::SourceCode);
}

// 48
#[test]
fn classify_source_code_js() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("app/index.js"), FileCategory::SourceCode);
}

// 49
#[test]
fn classify_config_cargo_toml() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("Cargo.toml"), FileCategory::Config);
}

// 50
#[test]
fn classify_config_yaml() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("config/app.yaml"), FileCategory::Config);
}

// 51
#[test]
fn classify_documentation_readme() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("README.md"), FileCategory::Documentation);
}

// 52
#[test]
fn classify_documentation_docs_dir() {
    let c = ChangeClassifier::new();
    assert_eq!(
        c.classify_path("docs/guide.md"),
        FileCategory::Documentation
    );
}

// 53
#[test]
fn classify_tests_dir() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("tests/integration.rs"), FileCategory::Tests);
}

// 54
#[test]
fn classify_tests_suffix() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("src/foo_test.rs"), FileCategory::Tests);
    assert_eq!(c.classify_path("src/bar.test.js"), FileCategory::Tests);
    assert_eq!(c.classify_path("src/baz.spec.ts"), FileCategory::Tests);
}

// 55
#[test]
fn classify_tests_prefix() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("test_utils.py"), FileCategory::Tests);
}

// 56
#[test]
fn classify_build_lock_file() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("Cargo.lock"), FileCategory::Build);
    assert_eq!(c.classify_path("package-lock.json"), FileCategory::Build);
}

// 57
#[test]
fn classify_cicd_github_workflows() {
    let c = ChangeClassifier::new();
    assert_eq!(
        c.classify_path(".github/workflows/ci.yml"),
        FileCategory::CiCd
    );
}

// 58
#[test]
fn classify_assets_image() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("logo.png"), FileCategory::Assets);
    assert_eq!(c.classify_path("icon.ico"), FileCategory::Assets);
}

// 59
#[test]
fn classify_other() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("unknown.xyz"), FileCategory::Other);
}

// 60
#[test]
fn classify_config_env_file() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("settings.env"), FileCategory::Config);
}

// ===========================================================================
// ChangeClassifier — is_security_sensitive
// ===========================================================================

// 61
#[test]
fn security_pem_file() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive("certs/server.pem"));
}

// 62
#[test]
fn security_key_file() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive("private.key"));
}

// 63
#[test]
fn security_env_file() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive(".env"));
    assert!(c.is_security_sensitive(".env.local"));
    assert!(c.is_security_sensitive(".env.production"));
}

// 64
#[test]
fn security_credentials_path() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive("config/credentials.json"));
}

// 65
#[test]
fn security_auth_file() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive("src/auth.rs"));
    assert!(c.is_security_sensitive("lib/oauth.py"));
}

// 66
#[test]
fn not_security_sensitive_regular_file() {
    let c = ChangeClassifier::new();
    assert!(!c.is_security_sensitive("src/lib.rs"));
    assert!(!c.is_security_sensitive("README.md"));
    assert!(!c.is_security_sensitive("index.html"));
}

// 67
#[test]
fn not_security_sensitive_authors_file() {
    let c = ChangeClassifier::new();
    assert!(!c.is_security_sensitive("AUTHORS"));
    assert!(!c.is_security_sensitive("AUTHORS.md"));
}

// 68
#[test]
fn security_secrets_dir() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive("config/secrets/db.yml"));
}

// 69
#[test]
fn security_ssh_dir() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive("home/.ssh/config"));
}

// 70
#[test]
fn security_token_file() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive("api_token.txt"));
}

// ===========================================================================
// ChangeClassifier — is_large_change
// ===========================================================================

// 71
#[test]
fn large_change_above_threshold() {
    let c = ChangeClassifier::new();
    assert!(c.is_large_change(300, 201));
}

// 72
#[test]
fn large_change_at_threshold() {
    let c = ChangeClassifier::new();
    assert!(!c.is_large_change(250, 250));
}

// 73
#[test]
fn large_change_below_threshold() {
    let c = ChangeClassifier::new();
    assert!(!c.is_large_change(100, 50));
}

// 74
#[test]
fn large_change_custom_threshold() {
    let c = ChangeClassifier::new().with_large_threshold(10);
    assert_eq!(c.large_change_threshold(), 10);
    assert!(c.is_large_change(6, 5));
    assert!(!c.is_large_change(5, 5));
}

// ===========================================================================
// DiffReport
// ===========================================================================

// 75
#[test]
fn report_empty_analysis() {
    let a = DiffAnalysis::default();
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    assert!(r.files.is_empty());
    assert_eq!(r.total_files, 0);
    assert_eq!(r.risk_level, RiskLevel::Low);
    assert!(r.summary_text.contains("No changes"));
    assert!(!r.has_security_sensitive_changes);
    assert_eq!(r.large_change_count, 0);
}

// 76
#[test]
fn report_single_source_file() {
    let raw = "\
diff --git a/src/lib.rs b/src/lib.rs
index abc1234..def5678 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1,2 @@
 fn a() {}
+fn b() {}
";
    let a = DiffAnalysis::parse(raw);
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    assert_eq!(r.total_files, 1);
    assert_eq!(r.files[0].category, FileCategory::SourceCode);
    assert_eq!(r.risk_level, RiskLevel::Low);
    assert!(!r.files[0].is_security_sensitive);
    assert!(!r.files[0].is_large);
}

// 77
#[test]
fn report_security_sensitive_raises_risk() {
    let raw = "\
diff --git a/.env b/.env
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/.env
@@ -0,0 +1 @@
+SECRET_KEY=abc123
";
    let a = DiffAnalysis::parse(raw);
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    assert_eq!(r.risk_level, RiskLevel::High);
    assert!(r.has_security_sensitive_changes);
    assert!(r.files[0].is_security_sensitive);
}

// 78
#[test]
fn report_large_change_medium_risk() {
    let mut lines = String::new();
    for i in 0..600 {
        lines.push_str(&format!("+line {i}\n"));
    }
    let raw = format!(
        "\
diff --git a/big.txt b/big.txt
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/big.txt
@@ -0,0 +1,600 @@
{lines}"
    );
    let a = DiffAnalysis::parse(&raw);
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    assert_eq!(r.risk_level, RiskLevel::Medium);
    assert!(r.files[0].is_large);
    assert_eq!(r.large_change_count, 1);
}

// 79
#[test]
fn report_binary_file_medium_risk() {
    let raw = "\
diff --git a/image.png b/image.png
new file mode 100644
index 0000000..abc1234
Binary files /dev/null and b/image.png differ
";
    let a = DiffAnalysis::parse(raw);
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    assert_eq!(r.risk_level, RiskLevel::Medium);
    assert!(r.files[0].is_binary);
}

// 80
#[test]
fn report_category_breakdown() {
    let raw = "\
diff --git a/src/lib.rs b/src/lib.rs
index abc1234..def5678 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1,2 @@
 fn a() {}
+fn b() {}
diff --git a/README.md b/README.md
index abc1234..def5678 100644
--- a/README.md
+++ b/README.md
@@ -1 +1,2 @@
 # Title
+More docs
diff --git a/Cargo.toml b/Cargo.toml
index abc1234..def5678 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1 +1,2 @@
 [package]
+name = \"foo\"
";
    let a = DiffAnalysis::parse(raw);
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    assert_eq!(r.categories.len(), 3);
    assert_eq!(r.categories[&FileCategory::SourceCode], 1);
    assert_eq!(r.categories[&FileCategory::Documentation], 1);
    assert_eq!(r.categories[&FileCategory::Config], 1);
}

// 81
#[test]
fn report_summary_text_format() {
    let raw = "\
diff --git a/a.txt b/a.txt
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/a.txt
@@ -0,0 +1 @@
+hello
diff --git a/b.txt b/b.txt
deleted file mode 100644
index abc1234..0000000
--- a/b.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
";
    let a = DiffAnalysis::parse(raw);
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    assert!(r.summary_text.contains("2 file(s) changed"));
    assert!(r.summary_text.contains("1 added"));
    assert!(r.summary_text.contains("1 deleted"));
    assert!(r.summary_text.contains("+1"));
    assert!(r.summary_text.contains("-1"));
    assert!(r.summary_text.contains("Risk:"));
}

// 82
#[test]
fn report_display_trait() {
    let a = DiffAnalysis::default();
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    let display = format!("{r}");
    assert_eq!(display, r.summary_text);
}

// 83
#[test]
fn report_serde_roundtrip() {
    let raw = "\
diff --git a/f.rs b/f.rs
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/f.rs
@@ -0,0 +1 @@
+fn main() {}
";
    let a = DiffAnalysis::parse(raw);
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    let json = serde_json::to_string(&r).unwrap();
    let rt: DiffReport = serde_json::from_str(&json).unwrap();
    assert_eq!(r, rt);
}

// ===========================================================================
// RiskLevel
// ===========================================================================

// 84
#[test]
fn risk_level_ordering() {
    assert!(RiskLevel::Low < RiskLevel::Medium);
    assert!(RiskLevel::Medium < RiskLevel::High);
}

// 85
#[test]
fn risk_level_display() {
    assert_eq!(format!("{}", RiskLevel::Low), "low");
    assert_eq!(format!("{}", RiskLevel::Medium), "medium");
    assert_eq!(format!("{}", RiskLevel::High), "high");
}

// 86
#[test]
fn risk_level_serde() {
    let json = serde_json::to_string(&RiskLevel::High).unwrap();
    assert_eq!(json, "\"high\"");
    let rt: RiskLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, RiskLevel::High);
}

// ===========================================================================
// DiffChangeKind
// ===========================================================================

// 87
#[test]
fn diff_change_kind_display() {
    assert_eq!(format!("{}", DiffChangeKind::Added), "added");
    assert_eq!(format!("{}", DiffChangeKind::Modified), "modified");
    assert_eq!(format!("{}", DiffChangeKind::Deleted), "deleted");
    assert_eq!(format!("{}", DiffChangeKind::Renamed), "renamed");
}

// 88
#[test]
fn diff_change_kind_serde() {
    let json = serde_json::to_string(&DiffChangeKind::Renamed).unwrap();
    assert_eq!(json, "\"renamed\"");
    let rt: DiffChangeKind = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, DiffChangeKind::Renamed);
}

// ===========================================================================
// FileCategory
// ===========================================================================

// 89
#[test]
fn file_category_display() {
    assert_eq!(format!("{}", FileCategory::SourceCode), "source code");
    assert_eq!(format!("{}", FileCategory::Config), "config");
    assert_eq!(format!("{}", FileCategory::Documentation), "documentation");
    assert_eq!(format!("{}", FileCategory::Tests), "tests");
    assert_eq!(format!("{}", FileCategory::Assets), "assets");
    assert_eq!(format!("{}", FileCategory::Build), "build");
    assert_eq!(format!("{}", FileCategory::CiCd), "ci/cd");
    assert_eq!(format!("{}", FileCategory::Other), "other");
}

// 90
#[test]
fn file_category_serde() {
    let json = serde_json::to_string(&FileCategory::CiCd).unwrap();
    assert_eq!(json, "\"ci_cd\"");
    let rt: FileCategory = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, FileCategory::CiCd);
}

// ===========================================================================
// FileType
// ===========================================================================

// 91
#[test]
fn file_type_display() {
    assert_eq!(format!("{}", FileType::Rust), "rust");
    assert_eq!(format!("{}", FileType::Binary), "binary");
    assert_eq!(format!("{}", FileType::Other), "other");
}

// 92
#[test]
fn file_type_serde() {
    let json = serde_json::to_string(&FileType::Python).unwrap();
    assert_eq!(json, "\"python\"");
    let rt: FileType = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, FileType::Python);
}

// ===========================================================================
// Edge cases
// ===========================================================================

// 93
#[test]
fn parse_garbage_input() {
    let a = DiffAnalysis::parse("this is not a diff\nrandom text\n");
    assert!(a.is_empty());
}

// 94
#[test]
fn parse_diff_header_only() {
    let raw = "diff --git a/f.txt b/f.txt\n";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert_eq!(a.files[0].change_kind, DiffChangeKind::Modified);
    assert_eq!(a.files[0].additions, 0);
}

// 95
#[test]
fn parse_empty_hunk() {
    let raw = "\
diff --git a/empty.txt b/empty.txt
index abc1234..def5678 100644
--- a/empty.txt
+++ b/empty.txt
@@ -0,0 +0,0 @@
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.file_count(), 1);
    assert_eq!(a.files[0].hunks.len(), 1);
    assert!(a.files[0].hunks[0].lines.is_empty());
}

// 96
#[test]
fn parse_nested_path() {
    let raw = "\
diff --git a/deeply/nested/dir/file.ts b/deeply/nested/dir/file.ts
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/deeply/nested/dir/file.ts
@@ -0,0 +1 @@
+export default {};
";
    let a = DiffAnalysis::parse(raw);
    assert_eq!(a.files[0].path, "deeply/nested/dir/file.ts");
    assert_eq!(a.files[0].file_type, FileType::TypeScript);
}

// 97
#[test]
fn report_renamed_files_in_summary() {
    let raw = "\
diff --git a/old.rs b/new.rs
similarity index 100%
rename from old.rs
rename to new.rs
";
    let a = DiffAnalysis::parse(raw);
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    assert!(r.summary_text.contains("1 renamed"));
}

// 98
#[test]
fn classifier_default_eq_new() {
    let a = ChangeClassifier::default();
    let b = ChangeClassifier::new();
    assert_eq!(a.large_change_threshold(), b.large_change_threshold());
}

// 99
#[test]
fn file_stats_serde_roundtrip() {
    let stats = FileStats {
        path: "src/main.rs".to_string(),
        additions: 10,
        deletions: 5,
        is_binary: false,
        file_type: FileType::Rust,
        change_kind: DiffChangeKind::Modified,
    };
    let json = serde_json::to_string(&stats).unwrap();
    let rt: FileStats = serde_json::from_str(&json).unwrap();
    assert_eq!(stats, rt);
}

// 100
#[test]
fn file_breakdown_serde_roundtrip() {
    let fb = FileBreakdown {
        path: "src/auth.rs".to_string(),
        change_kind: DiffChangeKind::Added,
        category: FileCategory::SourceCode,
        additions: 100,
        deletions: 0,
        is_binary: false,
        is_security_sensitive: true,
        is_large: false,
        risk: RiskLevel::High,
    };
    let json = serde_json::to_string(&fb).unwrap();
    let rt: FileBreakdown = serde_json::from_str(&json).unwrap();
    assert_eq!(fb, rt);
}

// 101
#[test]
fn report_max_risk_from_multiple_files() {
    let raw = "\
diff --git a/safe.txt b/safe.txt
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/safe.txt
@@ -0,0 +1 @@
+safe content
diff --git a/secrets/api.key b/secrets/api.key
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/secrets/api.key
@@ -0,0 +1 @@
+super_secret
";
    let a = DiffAnalysis::parse(raw);
    let c = ChangeClassifier::new();
    let r = DiffReport::from_analysis(&a, &c);
    // Overall risk should be High due to the secrets file
    assert_eq!(r.risk_level, RiskLevel::High);
    // But the safe file should be Low
    let safe_file = r.files.iter().find(|f| f.path == "safe.txt").unwrap();
    assert_eq!(safe_file.risk, RiskLevel::Low);
}

// 102
#[test]
fn classify_test_go_suffix() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path("pkg/handler_test.go"), FileCategory::Tests);
}

// 103
#[test]
fn classify_gitlab_ci() {
    let c = ChangeClassifier::new();
    assert_eq!(c.classify_path(".gitlab-ci.yml"), FileCategory::CiCd);
}

// 104
#[test]
fn security_private_key_path() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive("keys/private_key.pem"));
}

// 105
#[test]
fn security_p12_file() {
    let c = ChangeClassifier::new();
    assert!(c.is_security_sensitive("certs/client.p12"));
}
