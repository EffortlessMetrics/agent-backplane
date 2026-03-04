// SPDX-License-Identifier: MIT OR Apache-2.0
//! High-level vision emulation for text-only backends.
//!
//! [`VisionEmulator`] provides configurable fallback strategies when a backend
//! cannot process image inputs: placeholder text, user-supplied descriptions,
//! or outright removal.

use crate::strategies::VisionEmulation;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use serde::{Deserialize, Serialize};

/// Strategy for handling images on non-vision backends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VisionFallback {
    /// Replace images with `[Image N: ...]` placeholder text.
    Placeholder,
    /// Replace images with the given description text.
    Description {
        /// A textual description to substitute for the image.
        text: String,
    },
    /// Remove images entirely from the conversation.
    Remove,
    /// Return an error — refuse to process conversations containing images.
    Error,
}

/// Result of vision emulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisionEmulationResult {
    /// Number of images processed.
    pub images_processed: usize,
    /// The fallback strategy that was used.
    pub strategy_used: VisionFallback,
    /// Whether the conversation was modified.
    pub modified: bool,
}

/// High-level vision emulator with configurable fallback strategies.
#[derive(Debug, Clone)]
pub struct VisionEmulator {
    fallback: VisionFallback,
}

impl VisionEmulator {
    /// Create with the given fallback strategy.
    #[must_use]
    pub fn new(fallback: VisionFallback) -> Self {
        Self { fallback }
    }

    /// Create with the default placeholder strategy.
    #[must_use]
    pub fn placeholder() -> Self {
        Self::new(VisionFallback::Placeholder)
    }

    /// Create with a custom description for all images.
    #[must_use]
    pub fn with_description(text: impl Into<String>) -> Self {
        Self::new(VisionFallback::Description { text: text.into() })
    }

    /// The configured fallback strategy.
    #[must_use]
    pub fn fallback(&self) -> &VisionFallback {
        &self.fallback
    }

    /// Count the number of image blocks in a conversation.
    #[must_use]
    pub fn count_images(conv: &IrConversation) -> usize {
        conv.messages
            .iter()
            .flat_map(|m| &m.content)
            .filter(|b| matches!(b, IrContentBlock::Image { .. }))
            .count()
    }

    /// Apply the configured vision fallback to a conversation.
    ///
    /// Returns an error string if the strategy is [`VisionFallback::Error`]
    /// and images are present.
    pub fn apply(&self, conv: &mut IrConversation) -> Result<VisionEmulationResult, String> {
        let image_count = Self::count_images(conv);
        if image_count == 0 {
            return Ok(VisionEmulationResult {
                images_processed: 0,
                strategy_used: self.fallback.clone(),
                modified: false,
            });
        }

        match &self.fallback {
            VisionFallback::Placeholder => {
                let count = VisionEmulation::apply(conv);
                Ok(VisionEmulationResult {
                    images_processed: count,
                    strategy_used: self.fallback.clone(),
                    modified: true,
                })
            }
            VisionFallback::Description { text } => {
                let count = Self::replace_images_with_description(conv, text);
                Ok(VisionEmulationResult {
                    images_processed: count,
                    strategy_used: self.fallback.clone(),
                    modified: true,
                })
            }
            VisionFallback::Remove => {
                let count = Self::remove_images(conv);
                Ok(VisionEmulationResult {
                    images_processed: count,
                    strategy_used: self.fallback.clone(),
                    modified: true,
                })
            }
            VisionFallback::Error => Err(format!(
                "Conversation contains {image_count} image(s) but backend does not support vision"
            )),
        }
    }

    /// Replace all images with the given description text.
    fn replace_images_with_description(conv: &mut IrConversation, description: &str) -> usize {
        let mut count = 0;
        for msg in &mut conv.messages {
            let mut new_content = Vec::new();
            for block in &msg.content {
                match block {
                    IrContentBlock::Image { .. } => {
                        count += 1;
                        new_content.push(IrContentBlock::Text {
                            text: format!("[Image {count}: {description}]"),
                        });
                    }
                    other => new_content.push(other.clone()),
                }
            }
            msg.content = new_content;
        }
        count
    }

    /// Remove all image blocks entirely.
    fn remove_images(conv: &mut IrConversation) -> usize {
        let mut count = 0;
        for msg in &mut conv.messages {
            let before = msg.content.len();
            msg.content
                .retain(|b| !matches!(b, IrContentBlock::Image { .. }));
            count += before - msg.content.len();
        }
        count
    }

    /// Extract image metadata (media types) from a conversation.
    #[must_use]
    pub fn image_metadata(conv: &IrConversation) -> Vec<ImageMeta> {
        let mut meta = Vec::new();
        let mut idx = 0;
        for (msg_idx, msg) in conv.messages.iter().enumerate() {
            for block in &msg.content {
                if let IrContentBlock::Image { media_type, data } = block {
                    meta.push(ImageMeta {
                        index: idx,
                        message_index: msg_idx,
                        media_type: media_type.clone(),
                        data_length: data.len(),
                    });
                    idx += 1;
                }
            }
        }
        meta
    }
}

/// Metadata about a single image in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageMeta {
    /// Zero-based image index across the entire conversation.
    pub index: usize,
    /// Message index containing this image.
    pub message_index: usize,
    /// MIME type of the image.
    pub media_type: String,
    /// Length of the base64-encoded data.
    pub data_length: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_block(media: &str) -> IrContentBlock {
        IrContentBlock::Image {
            media_type: media.into(),
            data: "base64data".into(),
        }
    }

    fn conv_with_images() -> IrConversation {
        IrConversation::new()
            .push(IrMessage::new(
                IrRole::User,
                vec![
                    IrContentBlock::Text {
                        text: "Look at this".into(),
                    },
                    image_block("image/png"),
                ],
            ))
            .push(IrMessage::new(
                IrRole::User,
                vec![image_block("image/jpeg")],
            ))
    }

    #[test]
    fn count_images_zero() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "no images"));
        assert_eq!(VisionEmulator::count_images(&conv), 0);
    }

    #[test]
    fn count_images_multiple() {
        let conv = conv_with_images();
        assert_eq!(VisionEmulator::count_images(&conv), 2);
    }

    #[test]
    fn placeholder_fallback_replaces_images() {
        let emu = VisionEmulator::placeholder();
        let mut conv = conv_with_images();
        let result = emu.apply(&mut conv).unwrap();
        assert_eq!(result.images_processed, 2);
        assert!(result.modified);
        assert_eq!(VisionEmulator::count_images(&conv), 0);
    }

    #[test]
    fn description_fallback_inserts_text() {
        let emu = VisionEmulator::with_description("A diagram of a system");
        let mut conv = conv_with_images();
        let result = emu.apply(&mut conv).unwrap();
        assert_eq!(result.images_processed, 2);
        let text = conv.messages[0].text_content();
        assert!(text.contains("A diagram of a system"));
    }

    #[test]
    fn remove_fallback_drops_images() {
        let emu = VisionEmulator::new(VisionFallback::Remove);
        let mut conv = conv_with_images();
        let result = emu.apply(&mut conv).unwrap();
        assert_eq!(result.images_processed, 2);
        // Second message had only an image, now empty
        assert!(conv.messages[1].content.is_empty());
    }

    #[test]
    fn error_fallback_returns_error() {
        let emu = VisionEmulator::new(VisionFallback::Error);
        let mut conv = conv_with_images();
        let result = emu.apply(&mut conv);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("2 image(s)"));
    }

    #[test]
    fn error_fallback_ok_when_no_images() {
        let emu = VisionEmulator::new(VisionFallback::Error);
        let mut conv = IrConversation::new().push(IrMessage::text(IrRole::User, "text"));
        let result = emu.apply(&mut conv);
        assert!(result.is_ok());
        assert!(!result.unwrap().modified);
    }

    #[test]
    fn image_metadata_extraction() {
        let conv = conv_with_images();
        let meta = VisionEmulator::image_metadata(&conv);
        assert_eq!(meta.len(), 2);
        assert_eq!(meta[0].media_type, "image/png");
        assert_eq!(meta[0].message_index, 0);
        assert_eq!(meta[1].media_type, "image/jpeg");
        assert_eq!(meta[1].message_index, 1);
    }

    #[test]
    fn no_images_noop() {
        let emu = VisionEmulator::placeholder();
        let original = IrConversation::new().push(IrMessage::text(IrRole::User, "just text"));
        let mut conv = original.clone();
        let result = emu.apply(&mut conv).unwrap();
        assert_eq!(result.images_processed, 0);
        assert!(!result.modified);
        assert_eq!(conv, original);
    }

    #[test]
    fn serde_roundtrip_vision_fallback() {
        let strategies = vec![
            VisionFallback::Placeholder,
            VisionFallback::Description {
                text: "desc".into(),
            },
            VisionFallback::Remove,
            VisionFallback::Error,
        ];
        for s in &strategies {
            let json = serde_json::to_string(s).unwrap();
            let decoded: VisionFallback = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, decoded);
        }
    }
}
