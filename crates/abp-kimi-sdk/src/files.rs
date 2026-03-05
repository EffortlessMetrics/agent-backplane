// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kimi File API types for document parsing and file-based context.
//!
//! Kimi supports uploading files (PDF, DOCX, TXT, etc.) via the
//! `/v1/files` endpoint. Uploaded files can then be referenced in
//! conversations by including a `file_id` in a system message, allowing
//! the model to use the file's content as context.
//!
//! See <https://platform.moonshot.cn/docs/api/files>.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// File object
// ---------------------------------------------------------------------------

/// A file object returned by the Kimi Files API.
///
/// Represents an uploaded file that can be referenced in conversations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct KimiFile {
    /// Unique file identifier (e.g. `file-abc123`).
    pub id: String,
    /// Object type — always `"file"`.
    pub object: String,
    /// Size of the file in bytes.
    pub bytes: u64,
    /// Unix timestamp of when the file was created.
    pub created_at: u64,
    /// Original filename.
    pub filename: String,
    /// The intended purpose of the file (e.g. `"file-extract"`).
    pub purpose: String,
    /// Processing status of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<FileStatus>,
    /// Human-readable status message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_details: Option<String>,
}

/// Processing status of an uploaded file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    /// File is being processed.
    Uploaded,
    /// File has been processed and is ready for use.
    Processed,
    /// File processing encountered an error.
    Error,
}

// ---------------------------------------------------------------------------
// File list response
// ---------------------------------------------------------------------------

/// Response from `GET /v1/files`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct KimiFileList {
    /// Object type — always `"list"`.
    pub object: String,
    /// The list of file objects.
    pub data: Vec<KimiFile>,
}

// ---------------------------------------------------------------------------
// File content response
// ---------------------------------------------------------------------------

/// Response from `GET /v1/files/{file_id}/content`.
///
/// Contains the extracted text content from an uploaded file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct KimiFileContent {
    /// The extracted text content.
    pub content: String,
    /// The file ID this content belongs to.
    pub file_id: String,
    /// The content type (e.g. `"text/plain"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

// ---------------------------------------------------------------------------
// File deletion response
// ---------------------------------------------------------------------------

/// Response from `DELETE /v1/files/{file_id}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct KimiFileDeleted {
    /// The file identifier that was deleted.
    pub id: String,
    /// Object type — always `"file"`.
    pub object: String,
    /// Whether the deletion was successful.
    pub deleted: bool,
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

impl KimiFileList {
    /// Create a file list from a vec of files.
    #[must_use]
    pub fn new(files: Vec<KimiFile>) -> Self {
        Self {
            object: "list".into(),
            data: files,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file() -> KimiFile {
        KimiFile {
            id: "file-abc123".into(),
            object: "file".into(),
            bytes: 12345,
            created_at: 1700000000,
            filename: "report.pdf".into(),
            purpose: "file-extract".into(),
            status: Some(FileStatus::Processed),
            status_details: None,
        }
    }

    #[test]
    fn file_serde_roundtrip() {
        let file = sample_file();
        let json = serde_json::to_string(&file).unwrap();
        let parsed: KimiFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, file);
    }

    #[test]
    fn file_without_status_roundtrip() {
        let file = KimiFile {
            id: "file-xyz".into(),
            object: "file".into(),
            bytes: 1000,
            created_at: 1700000000,
            filename: "notes.txt".into(),
            purpose: "file-extract".into(),
            status: None,
            status_details: None,
        };
        let json = serde_json::to_string(&file).unwrap();
        assert!(!json.contains("status"));
        let parsed: KimiFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, file);
    }

    #[test]
    fn file_list_serde_roundtrip() {
        let list = KimiFileList::new(vec![sample_file()]);
        let json = serde_json::to_string(&list).unwrap();
        let parsed: KimiFileList = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.object, "list");
    }

    #[test]
    fn file_list_empty() {
        let list = KimiFileList::new(vec![]);
        let json = serde_json::to_string(&list).unwrap();
        let parsed: KimiFileList = serde_json::from_str(&json).unwrap();
        assert!(parsed.data.is_empty());
    }

    #[test]
    fn file_content_serde_roundtrip() {
        let content = KimiFileContent {
            content: "Extracted text from the PDF.".into(),
            file_id: "file-abc123".into(),
            content_type: Some("text/plain".into()),
        };
        let json = serde_json::to_string(&content).unwrap();
        let parsed: KimiFileContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, content);
    }

    #[test]
    fn file_deleted_serde_roundtrip() {
        let deleted = KimiFileDeleted {
            id: "file-abc123".into(),
            object: "file".into(),
            deleted: true,
        };
        let json = serde_json::to_string(&deleted).unwrap();
        let parsed: KimiFileDeleted = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, deleted);
        assert!(parsed.deleted);
    }

    #[test]
    fn file_status_serde_roundtrip() {
        for (status, expected) in [
            (FileStatus::Uploaded, "\"uploaded\""),
            (FileStatus::Processed, "\"processed\""),
            (FileStatus::Error, "\"error\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
            let parsed: FileStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn file_deserializes_from_api_json() {
        let json = r#"{
            "id": "file-abc123",
            "object": "file",
            "bytes": 52428,
            "created_at": 1715367049,
            "filename": "document.pdf",
            "purpose": "file-extract",
            "status": "processed"
        }"#;
        let file: KimiFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.id, "file-abc123");
        assert_eq!(file.filename, "document.pdf");
        assert_eq!(file.status, Some(FileStatus::Processed));
    }
}
