// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Extended multimodal content types for the Gemini API.
//!
//! Adds `FileData` (Google Cloud Storage references) and `VideoMetadata`
//! on top of the existing [`InlineData`](crate::gemini_types::InlineData).

use serde::{Deserialize, Serialize};

// ── FileData ────────────────────────────────────────────────────────────

/// A reference to a file stored in Google Cloud Storage or via the File API.
///
/// Corresponds to `fileData` in the Gemini REST API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    /// MIME type of the file (e.g. `"video/mp4"`, `"application/pdf"`).
    pub mime_type: String,
    /// URI of the file (e.g. `"gs://bucket/object"` or a File API URI).
    pub file_uri: String,
}

impl FileData {
    /// Create a new file data reference.
    #[must_use]
    pub fn new(mime_type: impl Into<String>, file_uri: impl Into<String>) -> Self {
        Self {
            mime_type: mime_type.into(),
            file_uri: file_uri.into(),
        }
    }

    /// Returns `true` if this references a Google Cloud Storage object.
    #[must_use]
    pub fn is_gcs(&self) -> bool {
        self.file_uri.starts_with("gs://")
    }
}

// ── VideoMetadata ───────────────────────────────────────────────────────

/// Metadata for video content describing the segment to process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    /// Start offset in the video as a duration string (e.g. `"10s"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<String>,
    /// End offset in the video as a duration string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_offset: Option<String>,
}

impl VideoMetadata {
    /// Create metadata for a full video (no offset constraints).
    #[must_use]
    pub fn full() -> Self {
        Self {
            start_offset: None,
            end_offset: None,
        }
    }

    /// Create metadata for a video segment.
    #[must_use]
    pub fn segment(start: impl Into<String>, end: impl Into<String>) -> Self {
        Self {
            start_offset: Some(start.into()),
            end_offset: Some(end.into()),
        }
    }
}

// ── Blob ────────────────────────────────────────────────────────────────

/// An inline binary blob — a thin wrapper around [`InlineData`](crate::gemini_types::InlineData)
/// with convenience constructors for common media types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Blob {
    /// MIME type (e.g. `"image/png"`, `"audio/wav"`).
    pub mime_type: String,
    /// Base64-encoded binary data.
    pub data: String,
}

impl Blob {
    /// Create a generic blob.
    #[must_use]
    pub fn new(mime_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            mime_type: mime_type.into(),
            data: data.into(),
        }
    }

    /// Create a PNG image blob.
    #[must_use]
    pub fn png(data: impl Into<String>) -> Self {
        Self::new("image/png", data)
    }

    /// Create a JPEG image blob.
    #[must_use]
    pub fn jpeg(data: impl Into<String>) -> Self {
        Self::new("image/jpeg", data)
    }

    /// Create a WebP image blob.
    #[must_use]
    pub fn webp(data: impl Into<String>) -> Self {
        Self::new("image/webp", data)
    }

    /// Create an audio WAV blob.
    #[must_use]
    pub fn wav(data: impl Into<String>) -> Self {
        Self::new("audio/wav", data)
    }

    /// Create a PDF blob.
    #[must_use]
    pub fn pdf(data: impl Into<String>) -> Self {
        Self::new("application/pdf", data)
    }

    /// Convert this blob into an [`InlineData`](crate::gemini_types::InlineData).
    #[must_use]
    pub fn into_inline_data(self) -> crate::gemini_types::InlineData {
        crate::gemini_types::InlineData {
            mime_type: self.mime_type,
            data: self.data,
        }
    }

    /// Convert this blob into a [`Part::InlineData`](crate::gemini_types::Part).
    #[must_use]
    pub fn into_part(self) -> crate::gemini_types::Part {
        crate::gemini_types::Part::InlineData(self.into_inline_data())
    }
}

impl From<Blob> for crate::gemini_types::InlineData {
    fn from(blob: Blob) -> Self {
        blob.into_inline_data()
    }
}

impl From<crate::gemini_types::InlineData> for Blob {
    fn from(data: crate::gemini_types::InlineData) -> Self {
        Self {
            mime_type: data.mime_type,
            data: data.data,
        }
    }
}

// ── Helper: classify MIME type ──────────────────────────────────────────

/// Broad media category for content routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCategory {
    /// Image content.
    Image,
    /// Audio content.
    Audio,
    /// Video content.
    Video,
    /// Document / text / PDF content.
    Document,
    /// Unrecognised media type.
    Unknown,
}

/// Classify a MIME type into a [`MediaCategory`].
#[must_use]
pub fn classify_mime(mime: &str) -> MediaCategory {
    if mime.starts_with("image/") {
        MediaCategory::Image
    } else if mime.starts_with("audio/") {
        MediaCategory::Audio
    } else if mime.starts_with("video/") {
        MediaCategory::Video
    } else if mime.starts_with("text/") || mime == "application/pdf" || mime == "application/json" {
        MediaCategory::Document
    } else {
        MediaCategory::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FileData ────────────────────────────────────────────────────

    #[test]
    fn file_data_serde_roundtrip() {
        let fd = FileData::new("video/mp4", "gs://bucket/video.mp4");
        let json = serde_json::to_string(&fd).unwrap();
        assert!(json.contains("mimeType"));
        assert!(json.contains("fileUri"));
        let back: FileData = serde_json::from_str(&json).unwrap();
        assert_eq!(fd, back);
    }

    #[test]
    fn file_data_gcs_detection() {
        let gcs = FileData::new("video/mp4", "gs://bucket/video.mp4");
        assert!(gcs.is_gcs());
        let non_gcs = FileData::new("video/mp4", "https://example.com/video.mp4");
        assert!(!non_gcs.is_gcs());
    }

    #[test]
    fn file_data_file_api_uri() {
        let fd = FileData::new(
            "application/pdf",
            "https://generativelanguage.googleapis.com/v1beta/files/abc123",
        );
        let json = serde_json::to_string(&fd).unwrap();
        let back: FileData = serde_json::from_str(&json).unwrap();
        assert_eq!(fd, back);
        assert!(!fd.is_gcs());
    }

    // ── VideoMetadata ───────────────────────────────────────────────

    #[test]
    fn video_metadata_full_roundtrip() {
        let vm = VideoMetadata::full();
        let json = serde_json::to_string(&vm).unwrap();
        assert_eq!(json, "{}");
        let back: VideoMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(vm, back);
    }

    #[test]
    fn video_metadata_segment_roundtrip() {
        let vm = VideoMetadata::segment("10s", "30s");
        let json = serde_json::to_string(&vm).unwrap();
        assert!(json.contains("startOffset"));
        assert!(json.contains("endOffset"));
        let back: VideoMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(vm, back);
    }

    #[test]
    fn video_metadata_start_only() {
        let vm = VideoMetadata {
            start_offset: Some("5s".into()),
            end_offset: None,
        };
        let json = serde_json::to_string(&vm).unwrap();
        assert!(json.contains("startOffset"));
        assert!(!json.contains("endOffset"));
        let back: VideoMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(vm, back);
    }

    // ── Blob ────────────────────────────────────────────────────────

    #[test]
    fn blob_serde_roundtrip() {
        let blob = Blob::new("image/png", "iVBORw0KGgo=");
        let json = serde_json::to_string(&blob).unwrap();
        let back: Blob = serde_json::from_str(&json).unwrap();
        assert_eq!(blob, back);
    }

    #[test]
    fn blob_convenience_constructors() {
        assert_eq!(Blob::png("data").mime_type, "image/png");
        assert_eq!(Blob::jpeg("data").mime_type, "image/jpeg");
        assert_eq!(Blob::webp("data").mime_type, "image/webp");
        assert_eq!(Blob::wav("data").mime_type, "audio/wav");
        assert_eq!(Blob::pdf("data").mime_type, "application/pdf");
    }

    #[test]
    fn blob_into_inline_data() {
        let blob = Blob::png("abc123");
        let inline = blob.clone().into_inline_data();
        assert_eq!(inline.mime_type, "image/png");
        assert_eq!(inline.data, "abc123");
    }

    #[test]
    fn blob_into_part() {
        use crate::gemini_types::Part;
        let part = Blob::jpeg("data").into_part();
        match &part {
            Part::InlineData(d) => {
                assert_eq!(d.mime_type, "image/jpeg");
                assert_eq!(d.data, "data");
            }
            _ => panic!("expected InlineData part"),
        }
    }

    #[test]
    fn blob_from_inline_data() {
        let inline = crate::gemini_types::InlineData {
            mime_type: "image/png".into(),
            data: "abc".into(),
        };
        let blob: Blob = inline.into();
        assert_eq!(blob.mime_type, "image/png");
        assert_eq!(blob.data, "abc");
    }

    #[test]
    fn inline_data_from_blob() {
        let blob = Blob::png("xyz");
        let inline: crate::gemini_types::InlineData = blob.into();
        assert_eq!(inline.mime_type, "image/png");
        assert_eq!(inline.data, "xyz");
    }

    // ── classify_mime ───────────────────────────────────────────────

    #[test]
    fn classify_mime_image() {
        assert_eq!(classify_mime("image/png"), MediaCategory::Image);
        assert_eq!(classify_mime("image/jpeg"), MediaCategory::Image);
        assert_eq!(classify_mime("image/webp"), MediaCategory::Image);
    }

    #[test]
    fn classify_mime_audio() {
        assert_eq!(classify_mime("audio/wav"), MediaCategory::Audio);
        assert_eq!(classify_mime("audio/mp3"), MediaCategory::Audio);
    }

    #[test]
    fn classify_mime_video() {
        assert_eq!(classify_mime("video/mp4"), MediaCategory::Video);
        assert_eq!(classify_mime("video/webm"), MediaCategory::Video);
    }

    #[test]
    fn classify_mime_document() {
        assert_eq!(classify_mime("text/plain"), MediaCategory::Document);
        assert_eq!(classify_mime("application/pdf"), MediaCategory::Document);
        assert_eq!(classify_mime("application/json"), MediaCategory::Document);
    }

    #[test]
    fn classify_mime_unknown() {
        assert_eq!(
            classify_mime("application/octet-stream"),
            MediaCategory::Unknown
        );
    }
}
