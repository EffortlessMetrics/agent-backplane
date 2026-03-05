// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for xtask subcommands.

use assert_cmd::Command;
use predicates::prelude::*;

#[allow(deprecated)] // cargo_bin works fine; the replacement macro is unstable
fn xtask() -> Command {
    Command::cargo_bin("xtask").unwrap()
}

#[test]
fn check_subcommand_exists() {
    xtask()
        .arg("check")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("CI"));
}

#[test]
fn lint_subcommand_exists() {
    xtask()
        .arg("lint")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("clippy").or(predicate::str::contains("formatting")));
}

#[test]
fn release_check_subcommand_exists() {
    xtask()
        .arg("release-check")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("release").or(predicate::str::contains("readiness")));
}

#[test]
fn docs_subcommand_exists() {
    xtask()
        .arg("docs")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("documentation"));
}

#[test]
fn docs_has_open_flag() {
    xtask()
        .arg("docs")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--open"));
}

#[test]
fn coverage_subcommand_exists() {
    xtask()
        .arg("coverage")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("coverage").or(predicate::str::contains("tarpaulin")));
}

#[test]
fn list_crates_produces_output() {
    xtask()
        .arg("list-crates")
        .assert()
        .success()
        .stdout(predicate::str::contains("abp-core"));
}

#[test]
fn schema_still_works() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    xtask()
        .args(["schema", "--out-dir"])
        .arg(tmp.path())
        .assert()
        .success();

    assert!(tmp.path().join("work_order.schema.json").exists());
    assert!(tmp.path().join("receipt.schema.json").exists());
}

#[test]
fn audit_subcommand_exists() {
    xtask()
        .arg("audit")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("quality").or(predicate::str::contains("unused")));
}

#[test]
fn stats_subcommand_exists() {
    xtask()
        .arg("stats")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("statistic").or(predicate::str::contains("LOC")));
}

#[test]
fn lint_fix_subcommand_exists() {
    xtask()
        .arg("lint-fix")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("fix").or(predicate::str::contains("format")));
}

#[test]
fn lint_fix_has_check_flag() {
    xtask()
        .arg("lint-fix")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--check"));
}

#[test]
fn gate_subcommand_exists() {
    xtask()
        .arg("gate")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("gate").or(predicate::str::contains("fmt")));
}

#[test]
fn gate_has_check_flag() {
    xtask()
        .arg("gate")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--check"));
}

#[test]
fn unknown_subcommand_errors() {
    xtask().arg("nonexistent-command").assert().failure();
}

#[test]
fn no_subcommand_shows_usage() {
    xtask()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn help_lists_all_subcommands() {
    xtask()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("schema"))
        .stdout(predicate::str::contains("audit"))
        .stdout(predicate::str::contains("stats"))
        .stdout(predicate::str::contains("setup"));
}

#[test]
fn stats_shows_expected_format() {
    xtask()
        .arg("stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("crates:"))
        .stdout(predicate::str::contains("LOC"))
        .stdout(predicate::str::contains("TOTAL"))
        .stdout(predicate::str::contains("dependency tree depth"));
}

#[test]
fn audit_runs_successfully() {
    xtask()
        .arg("audit")
        .assert()
        .success()
        .stdout(predicate::str::contains("required fields"))
        .stdout(predicate::str::contains("version consistency"))
        .stdout(predicate::str::contains("unused dependencies"));
}

#[test]
fn setup_subcommand_exists() {
    xtask()
        .arg("setup")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("hook").or(predicate::str::contains("development")));
}
