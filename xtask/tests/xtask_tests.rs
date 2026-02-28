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
        .stdout(predicate::str::contains("check").or(predicate::str::contains("Check")));
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
