// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for tool use, vision, thinking modules and translation.

use serde_json::json;

use claude_bridge::claude_types::*;
use claude_bridge::thinking::*;
use claude_bridge::tool_use::*;
use claude_bridge::vision::*;

// ═══════════════════════════════════════════════════════════════════
// 1. Tool use — InputSchema
// ═══════════════════════════════════════════════════════════════════

#[test]
fn input_schema_empty_object() {
    let s = InputSchema::empty();
    let v = s.to_value();
    assert_eq!(v["type"], "object");
    assert!(s.properties.is_empty());
    assert!(s.required.is_empty());
}

#[test]
fn input_schema_builder_chain() {
    let s = InputSchema::empty()
        .with_property("path", json!({"type": "string"}), true)
        .with_property("recursive", json!({"type": "boolean"}), false);
    assert_eq!(s.properties.len(), 2);
    assert_eq!(s.required, vec!["path"]);
    let v = s.to_value();
    assert_eq!(v["properties"]["recursive"]["type"], "boolean");
}

#[test]
fn input_schema_serde_roundtrip() {
    let s = InputSchema::empty().with_property("q", json!({"type": "string"}), true);
    let json = serde_json::to_string(&s).unwrap();
    let rt: InputSchema = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, s);
}

// ═══════════════════════════════════════════════════════════════════
// 2. Tool use — CachedToolDefinition
// ═══════════════════════════════════════════════════════════════════

#[test]
fn cached_tool_def_ephemeral() {
    let t = CachedToolDefinition::ephemeral("search", "Search files", json!({"type": "object"}));
    assert_eq!(t.cache_control.as_ref().unwrap().cache_type, "ephemeral");
}

#[test]
fn cached_tool_def_serde_roundtrip() {
    let t = CachedToolDefinition::ephemeral("x", "desc", json!({}));
    let v = serde_json::to_value(&t).unwrap();
    let rt: CachedToolDefinition = serde_json::from_value(v).unwrap();
    assert_eq!(rt, t);
}

#[test]
fn cached_tool_def_no_cache_omitted() {
    let t = CachedToolDefinition {
        name: "noop".into(),
        description: "d".into(),
        input_schema: json!({}),
        cache_control: None,
    };
    let v = serde_json::to_value(&t).unwrap();
    assert!(v.get("cache_control").is_none());
}

// ═══════════════════════════════════════════════════════════════════
// 3. Tool use — ToolResultContent
// ═══════════════════════════════════════════════════════════════════

#[test]
fn tool_result_content_text_serde() {
    let c = ToolResultContent::Text { text: "ok".into() };
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["type"], "text");
    let rt: ToolResultContent = serde_json::from_value(v).unwrap();
    assert_eq!(rt, c);
}

#[test]
fn tool_result_content_image_serde() {
    let c = ToolResultContent::Image {
        source: ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc".into(),
        },
    };
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["source"]["type"], "base64");
    let rt: ToolResultContent = serde_json::from_value(v).unwrap();
    assert_eq!(rt, c);
}

// ═══════════════════════════════════════════════════════════════════
// 4. Tool use — RichToolResult
// ═══════════════════════════════════════════════════════════════════

#[test]
fn rich_tool_result_text_shorthand() {
    let r = RichToolResult::text("tu_1", "result data");
    assert_eq!(r.tool_use_id, "tu_1");
    assert_eq!(r.text_content(), "result data");
    assert!(r.is_error.is_none());
}

#[test]
fn rich_tool_result_error_shorthand() {
    let r = RichToolResult::error("tu_2", "oops");
    assert_eq!(r.is_error, Some(true));
}

#[test]
fn rich_tool_result_with_image_parts() {
    let r = RichToolResult::with_image(
        "tu_3",
        "captured",
        ImageSource::Base64 {
            media_type: "image/jpeg".into(),
            data: "JFIF".into(),
        },
    );
    assert_eq!(r.content.len(), 2);
    assert_eq!(r.text_content(), "captured");
}

#[test]
fn rich_tool_result_empty_content() {
    let r = RichToolResult {
        tool_use_id: "tu_4".into(),
        content: vec![],
        is_error: None,
    };
    assert_eq!(r.text_content(), "");
}

#[test]
fn rich_tool_result_roundtrip() {
    let r = RichToolResult::text("tu_5", "data");
    let v = serde_json::to_value(&r).unwrap();
    let rt: RichToolResult = serde_json::from_value(v).unwrap();
    assert_eq!(rt, r);
}

// ═══════════════════════════════════════════════════════════════════
// 5. Vision — ImageMediaType
// ═══════════════════════════════════════════════════════════════════

#[test]
fn image_media_type_serde_roundtrip_all() {
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
fn image_media_type_from_mime_jpg_alias() {
    assert_eq!(
        ImageMediaType::from_mime("image/jpg"),
        Some(ImageMediaType::Jpeg)
    );
}

#[test]
fn image_media_type_from_mime_unknown() {
    assert!(ImageMediaType::from_mime("application/pdf").is_none());
}

#[test]
fn image_media_type_display() {
    assert_eq!(format!("{}", ImageMediaType::Webp), "image/webp");
}

// ═══════════════════════════════════════════════════════════════════
// 6. Vision — block builders
// ═══════════════════════════════════════════════════════════════════

#[test]
fn image_block_base64_builder() {
    let block = image_block_base64(ImageMediaType::Png, "iVBOR");
    match &block {
        ContentBlock::Image {
            source: ImageSource::Base64 { media_type, data },
        } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBOR");
        }
        _ => panic!("expected Image block"),
    }
}

#[test]
fn image_block_url_builder() {
    let block = image_block_url("https://example.com/x.png");
    match &block {
        ContentBlock::Image {
            source: ImageSource::Url { url },
        } => {
            assert_eq!(url, "https://example.com/x.png");
        }
        _ => panic!("expected Image/Url block"),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 7. Vision — validation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn validate_image_source_base64_ok() {
    let s = ImageSource::Base64 {
        media_type: "image/gif".into(),
        data: "R0lGODlh".into(),
    };
    assert!(validate_image_source(&s).is_ok());
}

#[test]
fn validate_image_source_unsupported_type() {
    let s = ImageSource::Base64 {
        media_type: "image/bmp".into(),
        data: "BM".into(),
    };
    let err = validate_image_source(&s).unwrap_err();
    assert!(err.contains("unsupported"));
}

#[test]
fn validate_image_source_empty_data() {
    let s = ImageSource::Base64 {
        media_type: "image/png".into(),
        data: "".into(),
    };
    assert!(validate_image_source(&s).is_err());
}

#[test]
fn validate_image_source_url_ok() {
    let s = ImageSource::Url {
        url: "https://example.com/img.webp".into(),
    };
    assert!(validate_image_source(&s).is_ok());
}

#[test]
fn validate_image_source_url_empty() {
    let s = ImageSource::Url { url: "".into() };
    assert!(validate_image_source(&s).is_err());
}

// ═══════════════════════════════════════════════════════════════════
// 8. Vision — extract_images
// ═══════════════════════════════════════════════════════════════════

#[test]
fn extract_images_mixed_blocks() {
    let blocks = vec![
        ContentBlock::Text { text: "a".into() },
        image_block_base64(ImageMediaType::Jpeg, "JFIF"),
        ContentBlock::ToolUse {
            id: "t1".into(),
            name: "x".into(),
            input: json!({}),
        },
        image_block_url("https://img.example.com/y.png"),
    ];
    assert_eq!(extract_images(&blocks).len(), 2);
}

#[test]
fn extract_images_none() {
    let blocks = vec![ContentBlock::Text {
        text: "text".into(),
    }];
    assert!(extract_images(&blocks).is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// 9. Thinking — ThinkingBlock
// ═══════════════════════════════════════════════════════════════════

#[test]
fn thinking_block_new_no_sig() {
    let tb = ThinkingBlock::new("pondering");
    assert!(!tb.has_signature());
    assert_eq!(tb.thinking, "pondering");
}

#[test]
fn thinking_block_with_sig() {
    let tb = ThinkingBlock::with_signature("deep thought", "sig_42");
    assert!(tb.has_signature());
}

#[test]
fn thinking_block_serde_roundtrip() {
    let tb = ThinkingBlock::with_signature("text", "sig");
    let json = serde_json::to_string(&tb).unwrap();
    let rt: ThinkingBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, tb);
}

#[test]
fn thinking_block_signature_omitted_when_none() {
    let tb = ThinkingBlock::new("hmm");
    let v = serde_json::to_value(&tb).unwrap();
    assert!(v.get("signature").is_none());
}

#[test]
fn thinking_block_to_content_block_roundtrip() {
    let tb = ThinkingBlock::with_signature("analysis", "s1");
    let cb = tb.to_content_block();
    let tb2 = ThinkingBlock::from_content_block(&cb).unwrap();
    assert_eq!(tb, tb2);
}

#[test]
fn thinking_block_from_non_thinking_none() {
    let cb = ContentBlock::Text { text: "hi".into() };
    assert!(ThinkingBlock::from_content_block(&cb).is_none());
}

// ═══════════════════════════════════════════════════════════════════
// 10. Thinking — SignatureBlock / deltas
// ═══════════════════════════════════════════════════════════════════

#[test]
fn signature_block_roundtrip() {
    let sb = SignatureBlock {
        signature: "abc123".into(),
    };
    let json = serde_json::to_string(&sb).unwrap();
    let rt: SignatureBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, sb);
}

#[test]
fn thinking_delta_to_stream_delta() {
    let td = ThinkingDelta {
        thinking: "chunk".into(),
    };
    let sd = td.to_stream_delta();
    assert!(matches!(sd, StreamDelta::ThinkingDelta { thinking } if thinking == "chunk"));
}

#[test]
fn signature_delta_to_stream_delta() {
    let sd = SignatureDelta {
        signature: "part".into(),
    };
    let d = sd.to_stream_delta();
    assert!(matches!(d, StreamDelta::SignatureDelta { signature } if signature == "part"));
}

// ═══════════════════════════════════════════════════════════════════
// 11. Thinking — config helpers
// ═══════════════════════════════════════════════════════════════════

#[test]
fn thinking_enabled_creates_correct_config() {
    let cfg = thinking_enabled(10000);
    assert_eq!(cfg.thinking_type, "enabled");
    assert_eq!(cfg.budget_tokens, 10000);
}

#[test]
fn thinking_disabled_creates_correct_config() {
    let cfg = thinking_disabled();
    assert_eq!(cfg.thinking_type, "disabled");
    assert_eq!(cfg.budget_tokens, 0);
}

#[test]
fn is_thinking_enabled_checks() {
    assert!(is_thinking_enabled(&thinking_enabled(5000)));
    assert!(!is_thinking_enabled(&thinking_disabled()));
    assert!(!is_thinking_enabled(&ThinkingConfig {
        thinking_type: "enabled".into(),
        budget_tokens: 0,
    }));
}

#[test]
fn validate_thinking_config_enabled_zero() {
    let cfg = ThinkingConfig {
        thinking_type: "enabled".into(),
        budget_tokens: 0,
    };
    assert!(validate_thinking_config(&cfg).is_err());
}

#[test]
fn validate_thinking_config_disabled_ok() {
    assert!(validate_thinking_config(&thinking_disabled()).is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// 12. Thinking — extract_thinking
// ═══════════════════════════════════════════════════════════════════

#[test]
fn extract_thinking_filters_correctly() {
    let blocks = vec![
        ContentBlock::Text { text: "a".into() },
        ContentBlock::Thinking {
            thinking: "step 1".into(),
            signature: Some("s1".into()),
        },
        ContentBlock::Thinking {
            thinking: "step 2".into(),
            signature: None,
        },
    ];
    let tbs = extract_thinking(&blocks);
    assert_eq!(tbs.len(), 2);
    assert!(tbs[0].has_signature());
    assert!(!tbs[1].has_signature());
}

#[test]
fn extract_thinking_empty_list() {
    assert!(extract_thinking(&[]).is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// 13. Translation — tool use roundtrips (feature-gated)
// ═══════════════════════════════════════════════════════════════════

#[cfg(feature = "normalized")]
mod translate_tests {
    use super::*;
    use abp_core::ir::*;
    use claude_bridge::translate::*;

    // ── Tool definition roundtrip ───────────────────────────────────

    #[test]
    fn tool_def_roundtrip() {
        let claude_td = ToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let ir = tool_def_to_ir(&claude_td);
        assert_eq!(ir.name, "read_file");
        let back = tool_def_from_ir(&ir);
        assert_eq!(back, claude_td);
    }

    #[test]
    fn tool_def_empty_schema_roundtrip() {
        let td = ToolDefinition {
            name: "noop".into(),
            description: "nothing".into(),
            input_schema: json!({}),
        };
        let ir = tool_def_to_ir(&td);
        let back = tool_def_from_ir(&ir);
        assert_eq!(back, td);
    }

    // ── Tool use block roundtrip ────────────────────────────────────

    #[test]
    fn tool_use_block_to_ir_and_back() {
        let block = ContentBlock::ToolUse {
            id: "toolu_01".into(),
            name: "search".into(),
            input: json!({"query": "rust"}),
        };
        let ir = content_block_to_ir(&block);
        assert!(matches!(&ir, IrContentBlock::ToolUse { name, .. } if name == "search"));
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    // ── Tool result block roundtrip ─────────────────────────────────

    #[test]
    fn tool_result_with_content_roundtrip() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_01".into(),
            content: Some("42 results found".into()),
            is_error: None,
        };
        let ir = content_block_to_ir(&block);
        match &ir {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_01");
                assert_eq!(content.len(), 1);
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    #[test]
    fn tool_result_empty_content_roundtrip() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_02".into(),
            content: None,
            is_error: None,
        };
        let ir = content_block_to_ir(&block);
        match &ir {
            IrContentBlock::ToolResult { content, .. } => {
                assert!(content.is_empty());
            }
            _ => panic!("expected ToolResult"),
        }
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    #[test]
    fn tool_result_error_roundtrip() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "toolu_03".into(),
            content: Some("permission denied".into()),
            is_error: Some(true),
        };
        let ir = content_block_to_ir(&block);
        match &ir {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("expected ToolResult"),
        }
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    // ── Rich tool result → IR ───────────────────────────────────────

    #[test]
    fn rich_tool_result_text_to_ir() {
        let r = RichToolResult::text("tu_1", "output");
        let ir = rich_tool_result_to_ir(&r);
        match &ir {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tu_1");
                assert_eq!(content.len(), 1);
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn rich_tool_result_with_image_to_ir() {
        let r = RichToolResult::with_image(
            "tu_2",
            "screenshot",
            ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc".into(),
            },
        );
        let ir = rich_tool_result_to_ir(&r);
        match &ir {
            IrContentBlock::ToolResult { content, .. } => {
                assert_eq!(content.len(), 2);
                assert!(
                    matches!(&content[0], IrContentBlock::Text { text } if text == "screenshot")
                );
                assert!(matches!(&content[1], IrContentBlock::Image { .. }));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn rich_tool_result_error_to_ir() {
        let r = RichToolResult::error("tu_3", "boom");
        let ir = rich_tool_result_to_ir(&r);
        match &ir {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn rich_tool_result_url_image_becomes_text() {
        let r = RichToolResult {
            tool_use_id: "tu_4".into(),
            content: vec![ToolResultContent::Image {
                source: ImageSource::Url {
                    url: "https://example.com/img.png".into(),
                },
            }],
            is_error: None,
        };
        let ir = rich_tool_result_to_ir(&r);
        match &ir {
            IrContentBlock::ToolResult { content, .. } => {
                assert!(
                    matches!(&content[0], IrContentBlock::Text { text } if text.contains("image:"))
                );
            }
            _ => panic!("expected ToolResult"),
        }
    }

    // ── Multi-tool call message ─────────────────────────────────────

    #[test]
    fn multi_tool_call_message_roundtrip() {
        let msg = Message {
            role: Role::Assistant,
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "I'll search.".into(),
                },
                ContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
                ContentBlock::ToolUse {
                    id: "tu_2".into(),
                    name: "read_file".into(),
                    input: json!({"path": "/main.rs"}),
                },
            ]),
        };
        let ir = message_to_ir(&msg);
        assert_eq!(ir.content.len(), 3);
        assert_eq!(ir.role, IrRole::Assistant);
        let back = message_from_ir(&ir);
        assert_eq!(back, msg);
    }

    // ── Cached tool definition → IR ─────────────────────────────────

    #[test]
    fn cached_tool_def_to_ir_drops_cache() {
        let ct = CachedToolDefinition::ephemeral("x", "desc", json!({"type": "object"}));
        let ir = cached_tool_def_to_ir(&ct);
        assert_eq!(ir.name, "x");
        assert_eq!(ir.description, "desc");
    }

    #[test]
    fn cached_tool_def_from_ir_no_cache() {
        let ir = IrToolDefinition {
            name: "y".into(),
            description: "d".into(),
            parameters: json!({}),
        };
        let ct = cached_tool_def_from_ir(&ir);
        assert!(ct.cache_control.is_none());
    }

    // ── Vision translation ──────────────────────────────────────────

    #[test]
    fn image_base64_to_ir_and_back() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "iVBOR".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        assert!(
            matches!(&ir, IrContentBlock::Image { media_type, .. } if media_type == "image/png")
        );
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    #[test]
    fn image_url_to_ir_becomes_text() {
        let block = ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/img.png".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        assert!(matches!(&ir, IrContentBlock::Text { text } if text.contains("image:")));
    }

    #[test]
    fn typed_image_to_ir_helper() {
        let ir = typed_image_to_ir(ImageMediaType::Jpeg, "JFIF");
        match &ir {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/jpeg");
                assert_eq!(data, "JFIF");
            }
            _ => panic!("expected Image"),
        }
    }

    // ── Thinking translation ────────────────────────────────────────

    #[test]
    fn thinking_block_to_ir_drops_signature() {
        let tb = ThinkingBlock::with_signature("deep thought", "sig_abc");
        let ir = thinking_block_to_ir(&tb);
        match &ir {
            IrContentBlock::Thinking { text } => assert_eq!(text, "deep thought"),
            _ => panic!("expected Thinking"),
        }
    }

    #[test]
    fn thinking_block_from_ir_no_signature() {
        let ir = IrContentBlock::Thinking {
            text: "reasoning".into(),
        };
        let tb = thinking_block_from_ir(&ir).unwrap();
        assert_eq!(tb.thinking, "reasoning");
        assert!(!tb.has_signature());
    }

    #[test]
    fn thinking_block_from_ir_non_thinking_none() {
        let ir = IrContentBlock::Text { text: "hi".into() };
        assert!(thinking_block_from_ir(&ir).is_none());
    }

    #[test]
    fn thinking_content_block_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "I think therefore I am".into(),
            signature: Some("sig123".into()),
        };
        let ir = content_block_to_ir(&block);
        match &ir {
            IrContentBlock::Thinking { text } => assert_eq!(text, "I think therefore I am"),
            _ => panic!("expected Thinking"),
        }
        // Back from IR: signature is lost
        let back = content_block_from_ir(&ir);
        match &back {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "I think therefore I am");
                assert!(signature.is_none());
            }
            _ => panic!("expected Thinking"),
        }
    }

    #[test]
    fn thinking_without_signature_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "hmm".into(),
            signature: None,
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert_eq!(back, block);
    }

    // ── ToolChoice / ThinkingConfig metadata roundtrips ─────────────

    #[test]
    fn tool_choice_auto_roundtrip_via_value() {
        let tc = ToolChoice::Auto {};
        let v = tool_choice_to_value(&tc);
        let rt = tool_choice_from_value(&v).unwrap();
        assert_eq!(rt, tc);
    }

    #[test]
    fn tool_choice_any_roundtrip_via_value() {
        let tc = ToolChoice::Any {};
        let v = tool_choice_to_value(&tc);
        let rt = tool_choice_from_value(&v).unwrap();
        assert_eq!(rt, tc);
    }

    #[test]
    fn tool_choice_specific_roundtrip_via_value() {
        let tc = ToolChoice::Tool {
            name: "search".into(),
        };
        let v = tool_choice_to_value(&tc);
        let rt = tool_choice_from_value(&v).unwrap();
        assert_eq!(rt, tc);
    }

    #[test]
    fn tool_choice_from_invalid_value_none() {
        assert!(tool_choice_from_value(&json!("garbage")).is_none());
    }

    #[test]
    fn thinking_config_roundtrip_via_value() {
        let cfg = thinking_enabled(8000);
        let v = thinking_config_to_value(&cfg);
        let rt = thinking_config_from_value(&v).unwrap();
        assert_eq!(rt, cfg);
    }

    #[test]
    fn thinking_config_from_invalid_value_none() {
        assert!(thinking_config_from_value(&json!(42)).is_none());
    }

    // ── Usage translation ───────────────────────────────────────────

    #[test]
    fn usage_to_ir_and_back_no_cache() {
        let u = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let ir = usage_to_ir(&u);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        let back = usage_from_ir(&ir);
        assert_eq!(back, u);
    }

    #[test]
    fn usage_to_ir_and_back_with_cache() {
        let u = Usage {
            input_tokens: 200,
            output_tokens: 80,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(30),
        };
        let ir = usage_to_ir(&u);
        assert_eq!(ir.cache_write_tokens, 10);
        assert_eq!(ir.cache_read_tokens, 30);
        let back = usage_from_ir(&ir);
        assert_eq!(back, u);
    }

    // ── Conversation with system message ────────────────────────────

    #[test]
    fn conversation_with_system_to_ir() {
        let sys = SystemMessage::Text("You are helpful.".into());
        let msgs = vec![Message {
            role: Role::User,
            content: MessageContent::Text("hello".into()),
        }];
        let conv = conversation_to_ir(&msgs, Some(&sys));
        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "You are helpful.");
    }
}
