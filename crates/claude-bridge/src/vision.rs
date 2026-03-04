// SPDX-License-Identifier: MIT OR Apache-2.0
//! Claude vision types and helpers.
//!
//! Extends [`ImageSource`](crate::claude_types::ImageSource) with a
//! typed media-type enum, validation, and convenient constructors for
//! building image content blocks.

use serde::{Deserialize, Serialize};

use crate::claude_types::{ContentBlock, ImageSource};

// ── Media types ─────────────────────────────────────────────────────────

/// Image media types supported by the Claude Messages API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageMediaType {
    /// `image/jpeg`
    #[serde(rename = "image/jpeg")]
    Jpeg,
    /// `image/png`
    #[serde(rename = "image/png")]
    Png,
    /// `image/gif`
    #[serde(rename = "image/gif")]
    Gif,
    /// `image/webp`
    #[serde(rename = "image/webp")]
    Webp,
}

impl ImageMediaType {
    /// Return the MIME string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    /// Try to parse a MIME string into an [`ImageMediaType`].
    #[must_use]
    pub fn from_mime(s: &str) -> Option<Self> {
        match s {
            "image/jpeg" | "image/jpg" => Some(Self::Jpeg),
            "image/png" => Some(Self::Png),
            "image/gif" => Some(Self::Gif),
            "image/webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

impl std::fmt::Display for ImageMediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Image content block builders ────────────────────────────────────────

/// Build a base64 image [`ContentBlock`] with a typed media type.
#[must_use]
pub fn image_block_base64(media_type: ImageMediaType, data: impl Into<String>) -> ContentBlock {
    ContentBlock::Image {
        source: ImageSource::Base64 {
            media_type: media_type.as_str().to_string(),
            data: data.into(),
        },
    }
}

/// Build a URL-referenced image [`ContentBlock`].
#[must_use]
pub fn image_block_url(url: impl Into<String>) -> ContentBlock {
    ContentBlock::Image {
        source: ImageSource::Url { url: url.into() },
    }
}

/// Validate that an [`ImageSource`] has a supported media type (for base64 sources).
///
/// Returns `Ok(())` if the source is a URL (no media type to check) or
/// if the media type is one of the four supported types.
pub fn validate_image_source(source: &ImageSource) -> Result<(), String> {
    match source {
        ImageSource::Base64 { media_type, data } => {
            if data.is_empty() {
                return Err("image data is empty".into());
            }
            if ImageMediaType::from_mime(media_type).is_none() {
                return Err(format!("unsupported media type: {media_type}"));
            }
            Ok(())
        }
        ImageSource::Url { url } => {
            if url.is_empty() {
                return Err("image URL is empty".into());
            }
            Ok(())
        }
    }
}

/// Extract all image blocks from a list of content blocks.
#[must_use]
pub fn extract_images(blocks: &[ContentBlock]) -> Vec<&ImageSource> {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Image { source } => Some(source),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn media_type_roundtrip() {
        for mt in [
            ImageMediaType::Jpeg,
            ImageMediaType::Png,
            ImageMediaType::Gif,
            ImageMediaType::Webp,
        ] {
            let json = serde_json::to_string(&mt).unwrap();
            let rt: ImageMediaType = serde_json::from_str(&json).unwrap();
            assert_eq!(rt, mt);
        }
    }

    #[test]
    fn media_type_as_str() {
        assert_eq!(ImageMediaType::Jpeg.as_str(), "image/jpeg");
        assert_eq!(ImageMediaType::Png.as_str(), "image/png");
        assert_eq!(ImageMediaType::Gif.as_str(), "image/gif");
        assert_eq!(ImageMediaType::Webp.as_str(), "image/webp");
    }

    #[test]
    fn media_type_from_mime() {
        assert_eq!(ImageMediaType::from_mime("image/jpeg"), Some(ImageMediaType::Jpeg));
        assert_eq!(ImageMediaType::from_mime("image/jpg"), Some(ImageMediaType::Jpeg));
        assert_eq!(ImageMediaType::from_mime("image/png"), Some(ImageMediaType::Png));
        assert_eq!(ImageMediaType::from_mime("image/gif"), Some(ImageMediaType::Gif));
        assert_eq!(ImageMediaType::from_mime("image/webp"), Some(ImageMediaType::Webp));
        assert_eq!(ImageMediaType::from_mime("image/tiff"), None);
        assert_eq!(ImageMediaType::from_mime("text/plain"), None);
    }

    #[test]
    fn media_type_display() {
        assert_eq!(ImageMediaType::Png.to_string(), "image/png");
    }

    #[test]
    fn image_block_base64_roundtrip() {
        let block = image_block_base64(ImageMediaType::Png, "iVBOR...");
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "image");
        assert_eq!(v["source"]["type"], "base64");
        assert_eq!(v["source"]["media_type"], "image/png");
        assert_eq!(v["source"]["data"], "iVBOR...");
        let rt: ContentBlock = serde_json::from_value(v).unwrap();
        assert_eq!(rt, block);
    }

    #[test]
    fn image_block_url_roundtrip() {
        let block = image_block_url("https://example.com/cat.jpg");
        let v = serde_json::to_value(&block).unwrap();
        assert_eq!(v["type"], "image");
        assert_eq!(v["source"]["type"], "url");
        assert_eq!(v["source"]["url"], "https://example.com/cat.jpg");
        let rt: ContentBlock = serde_json::from_value(v).unwrap();
        assert_eq!(rt, block);
    }

    #[test]
    fn validate_base64_ok() {
        let src = ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc".into(),
        };
        assert!(validate_image_source(&src).is_ok());
    }

    #[test]
    fn validate_base64_bad_type() {
        let src = ImageSource::Base64 {
            media_type: "image/tiff".into(),
            data: "abc".into(),
        };
        assert!(validate_image_source(&src).is_err());
    }

    #[test]
    fn validate_base64_empty_data() {
        let src = ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "".into(),
        };
        assert!(validate_image_source(&src).is_err());
    }

    #[test]
    fn validate_url_ok() {
        let src = ImageSource::Url {
            url: "https://example.com/img.png".into(),
        };
        assert!(validate_image_source(&src).is_ok());
    }

    #[test]
    fn validate_url_empty() {
        let src = ImageSource::Url { url: "".into() };
        assert!(validate_image_source(&src).is_err());
    }

    #[test]
    fn extract_images_filters_correctly() {
        let blocks = vec![
            ContentBlock::Text {
                text: "look at this".into(),
            },
            ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "abc".into(),
                },
            },
            ContentBlock::Text {
                text: "and this".into(),
            },
            ContentBlock::Image {
                source: ImageSource::Url {
                    url: "https://img.example.com/x.png".into(),
                },
            },
        ];
        let images = extract_images(&blocks);
        assert_eq!(images.len(), 2);
    }

    #[test]
    fn extract_images_empty() {
        let blocks = vec![ContentBlock::Text {
            text: "no images".into(),
        }];
        assert!(extract_images(&blocks).is_empty());
    }
}
