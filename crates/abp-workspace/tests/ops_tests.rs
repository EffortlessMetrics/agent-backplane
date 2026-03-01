// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for `abp_workspace::ops`.

use abp_workspace::ops::{FileOperation, OperationFilter, OperationLog, OperationSummary};

// ---------- FileOperation::paths ----------

#[test]
fn file_operation_read_paths() {
    let op = FileOperation::Read {
        path: "src/lib.rs".into(),
    };
    assert_eq!(op.paths(), vec!["src/lib.rs"]);
}

#[test]
fn file_operation_write_paths() {
    let op = FileOperation::Write {
        path: "out.txt".into(),
        size: 42,
    };
    assert_eq!(op.paths(), vec!["out.txt"]);
}

#[test]
fn file_operation_delete_paths() {
    let op = FileOperation::Delete {
        path: "tmp.log".into(),
    };
    assert_eq!(op.paths(), vec!["tmp.log"]);
}

#[test]
fn file_operation_move_paths() {
    let op = FileOperation::Move {
        from: "a.txt".into(),
        to: "b.txt".into(),
    };
    assert_eq!(op.paths(), vec!["a.txt", "b.txt"]);
}

#[test]
fn file_operation_copy_paths() {
    let op = FileOperation::Copy {
        from: "orig.rs".into(),
        to: "copy.rs".into(),
    };
    assert_eq!(op.paths(), vec!["orig.rs", "copy.rs"]);
}

#[test]
fn file_operation_create_dir_paths() {
    let op = FileOperation::CreateDir {
        path: "src/new".into(),
    };
    assert_eq!(op.paths(), vec!["src/new"]);
}

// ---------- OperationLog basics ----------

#[test]
fn operation_log_new_is_empty() {
    let log = OperationLog::new();
    assert!(log.operations().is_empty());
}

#[test]
fn operation_log_record_and_retrieve() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.txt".into(),
    });
    log.record(FileOperation::Delete {
        path: "b.txt".into(),
    });
    assert_eq!(log.operations().len(), 2);
}

// ---------- OperationLog query helpers ----------

#[test]
fn operation_log_reads() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "a.rs".into(),
    });
    log.record(FileOperation::Write {
        path: "b.rs".into(),
        size: 10,
    });
    log.record(FileOperation::Read {
        path: "c.rs".into(),
    });
    assert_eq!(log.reads(), vec!["a.rs", "c.rs"]);
}

#[test]
fn operation_log_writes() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Write {
        path: "x.rs".into(),
        size: 5,
    });
    log.record(FileOperation::Read {
        path: "y.rs".into(),
    });
    log.record(FileOperation::Write {
        path: "z.rs".into(),
        size: 15,
    });
    assert_eq!(log.writes(), vec!["x.rs", "z.rs"]);
}

#[test]
fn operation_log_deletes() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Delete {
        path: "old.txt".into(),
    });
    log.record(FileOperation::Read {
        path: "keep.txt".into(),
    });
    assert_eq!(log.deletes(), vec!["old.txt"]);
}

#[test]
fn operation_log_affected_paths_deduplicates() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read {
        path: "shared.rs".into(),
    });
    log.record(FileOperation::Write {
        path: "shared.rs".into(),
        size: 100,
    });
    log.record(FileOperation::Move {
        from: "shared.rs".into(),
        to: "new.rs".into(),
    });
    let paths = log.affected_paths();
    assert_eq!(paths.len(), 2);
    assert!(paths.contains("shared.rs"));
    assert!(paths.contains("new.rs"));
}

// ---------- OperationSummary ----------

#[test]
fn operation_log_summary_counts() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read { path: "a".into() });
    log.record(FileOperation::Write {
        path: "b".into(),
        size: 10,
    });
    log.record(FileOperation::Delete { path: "c".into() });
    log.record(FileOperation::Move {
        from: "d".into(),
        to: "e".into(),
    });
    log.record(FileOperation::Copy {
        from: "f".into(),
        to: "g".into(),
    });
    log.record(FileOperation::CreateDir { path: "h".into() });

    let s = log.summary();
    assert_eq!(
        s,
        OperationSummary {
            reads: 1,
            writes: 1,
            deletes: 1,
            moves: 1,
            copies: 1,
            create_dirs: 1,
            total_writes_bytes: 10,
        }
    );
}

#[test]
fn operation_log_summary_total_writes_bytes() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Write {
        path: "a".into(),
        size: 100,
    });
    log.record(FileOperation::Write {
        path: "b".into(),
        size: 250,
    });
    log.record(FileOperation::Write {
        path: "c".into(),
        size: 50,
    });
    assert_eq!(log.summary().total_writes_bytes, 400);
}

#[test]
fn operation_log_summary_empty() {
    let log = OperationLog::new();
    assert_eq!(log.summary(), OperationSummary::default());
}

// ---------- OperationLog::clear ----------

#[test]
fn operation_log_clear() {
    let mut log = OperationLog::new();
    log.record(FileOperation::Read { path: "x".into() });
    assert!(!log.operations().is_empty());
    log.clear();
    assert!(log.operations().is_empty());
}

// ---------- OperationFilter ----------

#[test]
fn filter_no_constraints_allows_all() {
    let f = OperationFilter::new();
    assert!(f.is_allowed("any/path.txt"));
    assert!(f.is_allowed("src/lib.rs"));
}

#[test]
fn filter_allowed_only() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("src/**");
    assert!(f.is_allowed("src/lib.rs"));
    assert!(!f.is_allowed("README.md"));
}

#[test]
fn filter_denied_only() {
    let mut f = OperationFilter::new();
    f.add_denied_path("*.log");
    assert!(!f.is_allowed("app.log"));
    assert!(f.is_allowed("src/main.rs"));
}

#[test]
fn filter_denied_takes_precedence() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("src/**");
    f.add_denied_path("src/secret/**");
    assert!(f.is_allowed("src/lib.rs"));
    assert!(!f.is_allowed("src/secret/key.pem"));
}

#[test]
fn filter_operations_returns_matching() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("src/**");

    let ops = vec![
        FileOperation::Read {
            path: "src/lib.rs".into(),
        },
        FileOperation::Read {
            path: "README.md".into(),
        },
        FileOperation::Write {
            path: "src/out.rs".into(),
            size: 10,
        },
    ];

    let allowed = f.filter_operations(&ops);
    assert_eq!(allowed.len(), 2);
    assert_eq!(allowed[0].paths(), vec!["src/lib.rs"]);
    assert_eq!(allowed[1].paths(), vec!["src/out.rs"]);
}

#[test]
fn filter_operations_move_requires_both_paths_allowed() {
    let mut f = OperationFilter::new();
    f.add_allowed_path("src/**");

    let ops = vec![
        FileOperation::Move {
            from: "src/a.rs".into(),
            to: "src/b.rs".into(),
        },
        FileOperation::Move {
            from: "src/c.rs".into(),
            to: "docs/c.rs".into(),
        },
    ];

    let allowed = f.filter_operations(&ops);
    assert_eq!(allowed.len(), 1);
    assert_eq!(allowed[0].paths(), vec!["src/a.rs", "src/b.rs"]);
}

// ---------- Serde round-trip ----------

#[test]
fn file_operation_serde_roundtrip() {
    let ops = vec![
        FileOperation::Read {
            path: "a.txt".into(),
        },
        FileOperation::Write {
            path: "b.txt".into(),
            size: 99,
        },
        FileOperation::Delete {
            path: "c.txt".into(),
        },
        FileOperation::Move {
            from: "d.txt".into(),
            to: "e.txt".into(),
        },
        FileOperation::Copy {
            from: "f.txt".into(),
            to: "g.txt".into(),
        },
        FileOperation::CreateDir { path: "h".into() },
    ];

    for op in &ops {
        let json = serde_json::to_string(op).expect("serialize");
        let back: FileOperation = serde_json::from_str(&json).expect("deserialize");
        // Compare debug representations for equality since we derive Debug but not PartialEq.
        assert_eq!(format!("{op:?}"), format!("{back:?}"));
    }
}
