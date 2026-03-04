// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for sidecar-kit: JSONL transport, framing, streaming,
//! error handling, concurrency, Unicode, and protocol integration.

use serde_json::{Value, json};
use sidecar_kit::{
    CancelToken, Frame, FrameReader, FrameWriter, JsonlCodec, ReceiptBuilder, SidecarError,
    buf_reader_from_bytes, frame_to_json, json_to_frame, read_all_frames, validate_frame,
    write_frames,
};

// ═══════════════════════════════════════════════════════════════════════
// 1. Value-based JSONL transport: serialize/deserialize serde_json::Value
// ═══════════════════════════════════════════════════════════════════════
mod value_transport {
    use super::*;

    #[test]
    fn serialize_hello_frame_to_value() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "test-backend"}),
            capabilities: json!({"tools": true}),
            mode: Value::Null,
        };
        let json_str = serde_json::to_string(&frame).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["t"], "hello");
        assert_eq!(v["contract_version"], "abp/v0.1");
    }

    #[test]
    fn deserialize_value_to_hello_frame() {
        let v = json!({"t": "hello", "contract_version": "abp/v0.1", "backend": {"id": "x"}, "capabilities": {}});
        let frame: Frame = serde_json::from_value(v).unwrap();
        match frame {
            Frame::Hello {
                contract_version, ..
            } => assert_eq!(contract_version, "abp/v0.1"),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn serialize_event_frame_with_nested_value() {
        let frame = Frame::Event {
            ref_id: "run-1".into(),
            event: json!({"type": "assistant_delta", "text": "hello", "nested": {"a": [1,2,3]}}),
        };
        let json_str = serde_json::to_string(&frame).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["event"]["nested"]["a"][1], 2);
    }

    #[test]
    fn serialize_final_frame_preserves_receipt_value() {
        let receipt = json!({"outcome": "complete", "tokens": 42});
        let frame = Frame::Final {
            ref_id: "r1".into(),
            receipt: receipt.clone(),
        };
        let json_str = serde_json::to_string(&frame).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["receipt"], receipt);
    }

    #[test]
    fn serialize_fatal_frame() {
        let frame = Frame::Fatal {
            ref_id: Some("r1".into()),
            error: "boom".into(),
        };
        let json_str = serde_json::to_string(&frame).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["t"], "fatal");
        assert_eq!(v["error"], "boom");
    }

    #[test]
    fn serialize_run_frame() {
        let frame = Frame::Run {
            id: "run-42".into(),
            work_order: json!({"task": "build"}),
        };
        let json_str = serde_json::to_string(&frame).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["t"], "run");
        assert_eq!(v["id"], "run-42");
    }

    #[test]
    fn serialize_cancel_frame() {
        let frame = Frame::Cancel {
            ref_id: "r1".into(),
            reason: Some("user requested".into()),
        };
        let json_str = serde_json::to_string(&frame).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["t"], "cancel");
        assert_eq!(v["reason"], "user requested");
    }

    #[test]
    fn serialize_ping_pong_frames() {
        let ping = Frame::Ping { seq: 7 };
        let pong = Frame::Pong { seq: 7 };
        let ps = serde_json::to_string(&ping).unwrap();
        let pp = serde_json::to_string(&pong).unwrap();
        let pv: Value = serde_json::from_str(&ps).unwrap();
        let ppv: Value = serde_json::from_str(&pp).unwrap();
        assert_eq!(pv["t"], "ping");
        assert_eq!(ppv["t"], "pong");
        assert_eq!(pv["seq"], 7);
    }

    #[test]
    fn cancel_frame_optional_reason_null() {
        let frame = Frame::Cancel {
            ref_id: "r2".into(),
            reason: None,
        };
        let json_str = serde_json::to_string(&frame).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        assert!(v["reason"].is_null());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Line framing: one JSON object per line, newline delimiter
// ═══════════════════════════════════════════════════════════════════════
mod line_framing {
    use super::*;

    #[test]
    fn codec_encode_appends_newline() {
        let frame = Frame::Ping { seq: 1 };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        assert!(encoded.ends_with('\n'));
        assert_eq!(encoded.matches('\n').count(), 1);
    }

    #[test]
    fn codec_encode_is_single_line() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"text": "line1\nline2"}),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        // The encoded JSON should escape the newline in the string value
        let lines: Vec<&str> = encoded.trim_end().split('\n').collect();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn codec_decode_trims_whitespace() {
        let json = r#"  {"t":"ping","seq":5}  "#;
        let frame = JsonlCodec::decode(json).unwrap();
        match frame {
            Frame::Ping { seq } => assert_eq!(seq, 5),
            _ => panic!("expected Ping"),
        }
    }

    #[test]
    fn codec_round_trip_all_frame_variants() {
        let frames = vec![
            Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: json!({"id": "b"}),
                capabilities: json!({}),
                mode: Value::Null,
            },
            Frame::Run {
                id: "r".into(),
                work_order: json!({}),
            },
            Frame::Event {
                ref_id: "r".into(),
                event: json!({"x": 1}),
            },
            Frame::Final {
                ref_id: "r".into(),
                receipt: json!({"ok": true}),
            },
            Frame::Fatal {
                ref_id: None,
                error: "err".into(),
            },
            Frame::Cancel {
                ref_id: "r".into(),
                reason: None,
            },
            Frame::Ping { seq: 0 },
            Frame::Pong { seq: 99 },
        ];
        for frame in &frames {
            let encoded = JsonlCodec::encode(frame).unwrap();
            let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
            let re_encoded = JsonlCodec::encode(&decoded).unwrap();
            assert_eq!(encoded, re_encoded);
        }
    }

    #[test]
    fn frame_to_json_does_not_have_trailing_newline() {
        let frame = Frame::Ping { seq: 3 };
        let json = frame_to_json(&frame).unwrap();
        assert!(!json.ends_with('\n'));
    }

    #[test]
    fn json_to_frame_parses_valid_json() {
        let json = r#"{"t":"pong","seq":10}"#;
        let frame = json_to_frame(json).unwrap();
        match frame {
            Frame::Pong { seq } => assert_eq!(seq, 10),
            _ => panic!("expected Pong"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Stream reading via FrameReader (sync BufRead)
// ═══════════════════════════════════════════════════════════════════════
mod stream_reading {
    use super::*;

    #[test]
    fn read_single_frame_from_bytes() {
        let data = b"{\"t\":\"ping\",\"seq\":1}\n";
        let mut reader = FrameReader::new(buf_reader_from_bytes(data));
        let frame = reader.read_frame().unwrap().unwrap();
        match frame {
            Frame::Ping { seq } => assert_eq!(seq, 1),
            _ => panic!("expected Ping"),
        }
        assert_eq!(reader.frames_read(), 1);
    }

    #[test]
    fn read_multiple_frames() {
        let data = b"{\"t\":\"ping\",\"seq\":1}\n{\"t\":\"pong\",\"seq\":1}\n";
        let mut reader = FrameReader::new(buf_reader_from_bytes(data));
        assert!(reader.read_frame().unwrap().is_some());
        assert!(reader.read_frame().unwrap().is_some());
        assert!(reader.read_frame().unwrap().is_none()); // EOF
        assert_eq!(reader.frames_read(), 2);
    }

    #[test]
    fn read_frame_returns_none_on_empty_input() {
        let data = b"";
        let mut reader = FrameReader::new(buf_reader_from_bytes(data));
        assert!(reader.read_frame().unwrap().is_none());
    }

    #[test]
    fn read_all_frames_collects_vec() {
        let data =
            b"{\"t\":\"ping\",\"seq\":1}\n{\"t\":\"ping\",\"seq\":2}\n{\"t\":\"ping\",\"seq\":3}\n";
        let frames = read_all_frames(buf_reader_from_bytes(data)).unwrap();
        assert_eq!(frames.len(), 3);
    }

    #[test]
    fn frame_reader_iterator_yields_all() {
        let data = b"{\"t\":\"ping\",\"seq\":10}\n{\"t\":\"pong\",\"seq\":10}\n";
        let reader = FrameReader::new(buf_reader_from_bytes(data));
        let frames: Vec<_> = reader.frames().collect::<Result<_, _>>().unwrap();
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn frame_reader_with_custom_max_size() {
        let data = b"{\"t\":\"ping\",\"seq\":1}\n";
        let mut reader = FrameReader::with_max_size(buf_reader_from_bytes(data), 1024);
        assert!(reader.read_frame().unwrap().is_some());
    }

    #[test]
    fn frame_reader_rejects_oversized_frame() {
        let data = b"{\"t\":\"ping\",\"seq\":1}\n";
        let mut reader = FrameReader::with_max_size(buf_reader_from_bytes(data), 5);
        let result = reader.read_frame();
        assert!(result.is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Stream writing via FrameWriter (sync Write, auto-flush)
// ═══════════════════════════════════════════════════════════════════════
mod stream_writing {
    use super::*;

    #[test]
    fn write_single_frame() {
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf);
        writer.write_frame(&Frame::Ping { seq: 1 }).unwrap();
        writer.flush().unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("\"seq\":1"));
        assert!(output.ends_with('\n'));
    }

    #[test]
    fn write_frame_increments_counter() {
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf);
        assert_eq!(writer.frames_written(), 0);
        writer.write_frame(&Frame::Ping { seq: 1 }).unwrap();
        assert_eq!(writer.frames_written(), 1);
        writer.write_frame(&Frame::Pong { seq: 1 }).unwrap();
        assert_eq!(writer.frames_written(), 2);
    }

    #[test]
    fn write_frame_respects_max_size() {
        let mut buf = Vec::new();
        let mut writer = FrameWriter::with_max_size(&mut buf, 5);
        let result = writer.write_frame(&Frame::Ping { seq: 1 });
        assert!(result.is_err());
    }

    #[test]
    fn write_frames_convenience_function() {
        let mut buf = Vec::new();
        let frames = vec![Frame::Ping { seq: 1 }, Frame::Pong { seq: 1 }];
        let count = write_frames(&mut buf, &frames).unwrap();
        assert_eq!(count, 2);
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn writer_inner_and_into_inner() {
        let buf = Vec::new();
        let writer = FrameWriter::new(buf);
        let _ = writer.inner();
        let recovered = writer.into_inner();
        assert!(recovered.is_empty());
    }

    #[test]
    fn write_multiple_frames_each_on_own_line() {
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf);
        for i in 0..5 {
            writer.write_frame(&Frame::Ping { seq: i }).unwrap();
        }
        writer.flush().unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim_end().split('\n').collect();
        assert_eq!(lines.len(), 5);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Error handling: malformed JSON, IO errors, EOF
// ═══════════════════════════════════════════════════════════════════════
mod error_handling {
    use super::*;

    #[test]
    fn decode_malformed_json_returns_error() {
        let result = JsonlCodec::decode("not valid json{{{");
        assert!(result.is_err());
    }

    #[test]
    fn decode_empty_string_returns_error() {
        let result = JsonlCodec::decode("");
        assert!(result.is_err());
    }

    #[test]
    fn decode_partial_json_returns_error() {
        let result = JsonlCodec::decode(r#"{"t":"ping","seq":"#);
        assert!(result.is_err());
    }

    #[test]
    fn decode_unknown_tag_returns_error() {
        let result = JsonlCodec::decode(r#"{"t":"unknown_tag","foo":1}"#);
        assert!(result.is_err());
    }

    #[test]
    fn decode_missing_tag_returns_error() {
        let result = JsonlCodec::decode(r#"{"seq":1}"#);
        assert!(result.is_err());
    }

    #[test]
    fn reader_malformed_line_returns_error() {
        let data = b"this is not json\n";
        let mut reader = FrameReader::new(buf_reader_from_bytes(data));
        assert!(reader.read_frame().is_err());
    }

    #[test]
    fn reader_first_valid_second_malformed() {
        let data = b"{\"t\":\"ping\",\"seq\":1}\nnot json\n";
        let mut reader = FrameReader::new(buf_reader_from_bytes(data));
        assert!(reader.read_frame().unwrap().is_some());
        assert!(reader.read_frame().is_err());
    }

    #[test]
    fn sidecar_error_display_protocol() {
        let err = SidecarError::Protocol("test error".into());
        let msg = format!("{err}");
        assert!(msg.contains("test error"));
    }

    #[test]
    fn sidecar_error_display_fatal() {
        let err = SidecarError::Fatal("something broke".into());
        let msg = format!("{err}");
        assert!(msg.contains("something broke"));
    }

    #[test]
    fn sidecar_error_display_timeout() {
        let err = SidecarError::Timeout;
        let msg = format!("{err}");
        assert!(msg.contains("timed out"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Large payloads: multi-KB JSON objects
// ═══════════════════════════════════════════════════════════════════════
mod large_payloads {
    use super::*;

    #[test]
    fn encode_decode_large_event_payload() {
        let big_text = "x".repeat(10_000);
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"type": "assistant_delta", "text": big_text}),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => {
                assert_eq!(event["text"].as_str().unwrap().len(), 10_000);
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn write_read_large_frame_round_trip() {
        let big_data: Vec<Value> = (0..500)
            .map(|i| json!({"index": i, "data": "a]".repeat(20)}))
            .collect();
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"items": big_data}),
        };
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf);
        writer.write_frame(&frame).unwrap();
        writer.flush().unwrap();

        let mut reader = FrameReader::new(buf_reader_from_bytes(&buf));
        let read_frame = reader.read_frame().unwrap().unwrap();
        let orig_json = serde_json::to_string(&frame).unwrap();
        let read_json = serde_json::to_string(&read_frame).unwrap();
        assert_eq!(orig_json, read_json);
    }

    #[test]
    fn large_receipt_value_survives_transport() {
        let receipt = ReceiptBuilder::new("run-big", "backend-1")
            .event(json!({"big": "a]".repeat(5000)}))
            .event(json!({"another": "b".repeat(5000)}))
            .input_tokens(100_000)
            .output_tokens(50_000)
            .build();
        let frame = Frame::Final {
            ref_id: "run-big".into(),
            receipt: receipt.clone(),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Final { receipt: r, .. } => {
                assert_eq!(r["usage"]["input_tokens"], 100_000);
            }
            _ => panic!("expected Final"),
        }
    }

    #[test]
    fn frame_exceeding_max_size_rejected_by_writer() {
        let big = "y".repeat(200);
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"text": big}),
        };
        let mut buf = Vec::new();
        let mut writer = FrameWriter::with_max_size(&mut buf, 100);
        assert!(writer.write_frame(&frame).is_err());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Empty lines: skip gracefully
// ═══════════════════════════════════════════════════════════════════════
mod empty_lines {
    use super::*;

    #[test]
    fn reader_skips_empty_lines() {
        let data = b"\n\n{\"t\":\"ping\",\"seq\":1}\n\n\n";
        let mut reader = FrameReader::new(buf_reader_from_bytes(data));
        let frame = reader.read_frame().unwrap().unwrap();
        match frame {
            Frame::Ping { seq } => assert_eq!(seq, 1),
            _ => panic!("expected Ping"),
        }
        assert!(reader.read_frame().unwrap().is_none()); // EOF after trailing empties
    }

    #[test]
    fn reader_skips_whitespace_only_lines() {
        let data = b"   \n  \t  \n{\"t\":\"pong\",\"seq\":2}\n";
        let mut reader = FrameReader::new(buf_reader_from_bytes(data));
        let frame = reader.read_frame().unwrap().unwrap();
        match frame {
            Frame::Pong { seq } => assert_eq!(seq, 2),
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn reader_handles_only_empty_lines() {
        let data = b"\n\n\n\n";
        let mut reader = FrameReader::new(buf_reader_from_bytes(data));
        assert!(reader.read_frame().unwrap().is_none());
    }

    #[test]
    fn reader_empty_lines_between_frames() {
        let data = b"{\"t\":\"ping\",\"seq\":1}\n\n\n{\"t\":\"ping\",\"seq\":2}\n";
        let frames = read_all_frames(buf_reader_from_bytes(data)).unwrap();
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn reader_frames_count_excludes_empty_lines() {
        let data = b"\n{\"t\":\"ping\",\"seq\":1}\n\n{\"t\":\"ping\",\"seq\":2}\n\n";
        let mut reader = FrameReader::new(buf_reader_from_bytes(data));
        while reader.read_frame().unwrap().is_some() {}
        assert_eq!(reader.frames_read(), 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Concurrent read/write via tokio channels
// ═══════════════════════════════════════════════════════════════════════
mod concurrent_rw {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn channel_based_frame_exchange() {
        let (tx, mut rx) = mpsc::channel::<Frame>(16);
        let producer = tokio::spawn(async move {
            for i in 0..10u64 {
                tx.send(Frame::Ping { seq: i }).await.unwrap();
            }
        });
        let consumer = tokio::spawn(async move {
            let mut received = Vec::new();
            while let Some(frame) = rx.recv().await {
                received.push(frame);
            }
            received
        });
        producer.await.unwrap();
        let received = consumer.await.unwrap();
        assert_eq!(received.len(), 10);
    }

    #[tokio::test]
    async fn channel_serialized_frame_exchange() {
        let (tx, mut rx) = mpsc::channel::<String>(16);
        let producer = tokio::spawn(async move {
            for i in 0..5u64 {
                let frame = Frame::Ping { seq: i };
                let line = JsonlCodec::encode(&frame).unwrap();
                tx.send(line).await.unwrap();
            }
        });
        let consumer = tokio::spawn(async move {
            let mut frames = Vec::new();
            while let Some(line) = rx.recv().await {
                let frame = JsonlCodec::decode(line.trim()).unwrap();
                frames.push(frame);
            }
            frames
        });
        producer.await.unwrap();
        let frames = consumer.await.unwrap();
        assert_eq!(frames.len(), 5);
    }

    #[tokio::test]
    async fn multiple_producers_single_consumer() {
        let (tx, mut rx) = mpsc::channel::<Frame>(32);
        for i in 0..4 {
            let tx = tx.clone();
            tokio::spawn(async move {
                for j in 0..5u64 {
                    tx.send(Frame::Ping { seq: i * 10 + j }).await.unwrap();
                }
            });
        }
        drop(tx);
        let mut count = 0u64;
        while let Some(_frame) = rx.recv().await {
            count += 1;
        }
        assert_eq!(count, 20);
    }

    #[tokio::test]
    async fn cancel_token_stops_producer() {
        let (tx, mut rx) = mpsc::channel::<Frame>(64);
        let cancel = CancelToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            let mut seq = 0u64;
            loop {
                if cancel_clone.is_cancelled() {
                    break;
                }
                let _ = tx.send(Frame::Ping { seq }).await;
                seq += 1;
                tokio::task::yield_now().await;
            }
        });
        // Collect some frames then cancel
        let mut count = 0;
        while let Some(_f) = rx.recv().await {
            count += 1;
            if count >= 5 {
                cancel.cancel();
                break;
            }
        }
        assert!(count >= 5);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Backpressure: slow reader, fast writer
// ═══════════════════════════════════════════════════════════════════════
mod backpressure {
    use super::*;
    use tokio::sync::mpsc;
    use tokio::time::{Duration, timeout};

    #[tokio::test]
    async fn bounded_channel_blocks_fast_writer() {
        let (tx, mut rx) = mpsc::channel::<Frame>(2);
        let writer = tokio::spawn(async move {
            let mut sent = 0u64;
            for i in 0..10u64 {
                if tx.send(Frame::Ping { seq: i }).await.is_ok() {
                    sent += 1;
                }
            }
            sent
        });
        // Read slowly
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut received = 0u64;
        while let Ok(Some(_)) = timeout(Duration::from_millis(200), rx.recv()).await {
            received += 1;
        }
        let sent = writer.await.unwrap();
        assert_eq!(sent, 10);
        assert_eq!(received, 10);
    }

    #[tokio::test]
    async fn backpressure_does_not_lose_frames() {
        let (tx, mut rx) = mpsc::channel::<String>(1);
        let producer = tokio::spawn(async move {
            for i in 0..20u64 {
                let frame = Frame::Ping { seq: i };
                let line = JsonlCodec::encode(&frame).unwrap();
                tx.send(line).await.unwrap();
            }
        });
        let consumer = tokio::spawn(async move {
            let mut frames = Vec::new();
            while let Some(line) = rx.recv().await {
                // Simulate slow processing
                tokio::task::yield_now().await;
                let frame = JsonlCodec::decode(line.trim()).unwrap();
                frames.push(frame);
            }
            frames
        });
        producer.await.unwrap();
        let frames = consumer.await.unwrap();
        assert_eq!(frames.len(), 20);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Unicode handling: multi-byte chars in JSON strings
// ═══════════════════════════════════════════════════════════════════════
mod unicode_handling {
    use super::*;

    #[test]
    fn unicode_in_event_text() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"text": "こんにちは世界 🌍"}),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => {
                assert_eq!(event["text"], "こんにちは世界 🌍");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn unicode_in_fatal_error_message() {
        let frame = Frame::Fatal {
            ref_id: None,
            error: "Ошибка: файл не найден 📁".into(),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Fatal { error, .. } => {
                assert!(error.contains("Ошибка"));
                assert!(error.contains("📁"));
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn unicode_in_backend_id() {
        let frame = Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "バックエンド-1"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Hello { backend, .. } => {
                assert_eq!(backend["id"], "バックエンド-1");
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn unicode_write_read_round_trip() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"emoji": "🎉🎊🎈", "chinese": "你好", "arabic": "مرحبا"}),
        };
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf);
        writer.write_frame(&frame).unwrap();
        writer.flush().unwrap();

        let mut reader = FrameReader::new(buf_reader_from_bytes(&buf));
        let read_frame = reader.read_frame().unwrap().unwrap();
        match read_frame {
            Frame::Event { event, .. } => {
                assert_eq!(event["emoji"], "🎉🎊🎈");
                assert_eq!(event["chinese"], "你好");
                assert_eq!(event["arabic"], "مرحبا");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn unicode_escape_sequences_in_json() {
        let json_str =
            r#"{"t":"event","ref_id":"r1","event":{"text":"\u0048\u0065\u006C\u006C\u006F"}}"#;
        let frame = JsonlCodec::decode(json_str).unwrap();
        match frame {
            Frame::Event { event, .. } => {
                assert_eq!(event["text"], "Hello");
            }
            _ => panic!("expected Event"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Roundtrip: write → read preserves exact JSON
// ═══════════════════════════════════════════════════════════════════════
mod roundtrip {
    use super::*;

    fn roundtrip_frame(frame: &Frame) {
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf);
        writer.write_frame(frame).unwrap();
        writer.flush().unwrap();

        let mut reader = FrameReader::new(buf_reader_from_bytes(&buf));
        let read_frame = reader.read_frame().unwrap().unwrap();
        let original = serde_json::to_value(frame).unwrap();
        let recovered = serde_json::to_value(&read_frame).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn roundtrip_hello() {
        roundtrip_frame(&Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "test", "version": "1.0"}),
            capabilities: json!({"tools": true, "streaming": true}),
            mode: json!("mapped"),
        });
    }

    #[test]
    fn roundtrip_run() {
        roundtrip_frame(&Frame::Run {
            id: "run-abc-123".into(),
            work_order: json!({"task": "build project", "context": {"files": ["a.rs", "b.rs"]}}),
        });
    }

    #[test]
    fn roundtrip_event() {
        roundtrip_frame(&Frame::Event {
            ref_id: "run-1".into(),
            event: json!({"type": "assistant_delta", "text": "Hello!", "ts": "2024-01-01T00:00:00Z"}),
        });
    }

    #[test]
    fn roundtrip_final() {
        roundtrip_frame(&Frame::Final {
            ref_id: "run-1".into(),
            receipt: json!({"outcome": "complete", "usage": {"input_tokens": 100}}),
        });
    }

    #[test]
    fn roundtrip_fatal() {
        roundtrip_frame(&Frame::Fatal {
            ref_id: Some("run-1".into()),
            error: "something went terribly wrong".into(),
        });
    }

    #[test]
    fn roundtrip_cancel() {
        roundtrip_frame(&Frame::Cancel {
            ref_id: "run-1".into(),
            reason: Some("user pressed ctrl-c".into()),
        });
    }

    #[test]
    fn roundtrip_ping_pong() {
        roundtrip_frame(&Frame::Ping { seq: 42 });
        roundtrip_frame(&Frame::Pong { seq: 42 });
    }

    #[test]
    fn roundtrip_multiple_frames_preserves_order() {
        let frames = vec![
            Frame::Ping { seq: 1 },
            Frame::Ping { seq: 2 },
            Frame::Ping { seq: 3 },
            Frame::Pong { seq: 3 },
        ];
        let mut buf = Vec::new();
        write_frames(&mut buf, &frames).unwrap();

        let read_frames = read_all_frames(buf_reader_from_bytes(&buf)).unwrap();
        assert_eq!(read_frames.len(), frames.len());
        for (orig, read) in frames.iter().zip(read_frames.iter()) {
            assert_eq!(
                serde_json::to_value(orig).unwrap(),
                serde_json::to_value(read).unwrap()
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Integration with ABP protocol types (Envelope as Value)
// ═══════════════════════════════════════════════════════════════════════
mod abp_integration {
    use super::*;
    use sidecar_kit::builders::*;
    use sidecar_kit::{ProtocolPhase, ProtocolState};

    #[test]
    fn hello_frame_builder_produces_valid_frame() {
        let frame = hello_frame("my-backend");
        match &frame {
            Frame::Hello {
                contract_version,
                backend,
                ..
            } => {
                assert_eq!(contract_version, "abp/v0.1");
                assert_eq!(backend["id"], "my-backend");
            }
            _ => panic!("expected Hello"),
        }
        let validation = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(validation.valid, "issues: {:?}", validation.issues);
    }

    #[test]
    fn event_frame_builder_wraps_event_value() {
        let ev = event_text_delta("hello world");
        let frame = event_frame("run-1", ev);
        match &frame {
            Frame::Event { ref_id, event } => {
                assert_eq!(ref_id, "run-1");
                assert_eq!(event["type"], "assistant_delta");
                assert_eq!(event["text"], "hello world");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn fatal_frame_builder() {
        let frame = fatal_frame(Some("run-1"), "oops");
        match &frame {
            Frame::Fatal { ref_id, error } => {
                assert_eq!(ref_id.as_deref(), Some("run-1"));
                assert_eq!(error, "oops");
            }
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn receipt_builder_default_outcome_is_complete() {
        let receipt = ReceiptBuilder::new("run-1", "backend-1").build();
        assert_eq!(receipt["outcome"], "complete");
        assert_eq!(receipt["meta"]["contract_version"], "abp/v0.1");
    }

    #[test]
    fn receipt_builder_failed_outcome() {
        let receipt = ReceiptBuilder::new("r", "b").failed().build();
        assert_eq!(receipt["outcome"], "failed");
    }

    #[test]
    fn receipt_builder_partial_outcome() {
        let receipt = ReceiptBuilder::new("r", "b").partial().build();
        assert_eq!(receipt["outcome"], "partial");
    }

    #[test]
    fn receipt_builder_with_events_and_artifacts() {
        let receipt = ReceiptBuilder::new("r1", "b1")
            .event(event_text_message("done"))
            .artifact("patch", "output.diff")
            .input_tokens(500)
            .output_tokens(200)
            .usage_raw(json!({"model": "gpt-4"}))
            .build();
        assert_eq!(receipt["trace"].as_array().unwrap().len(), 1);
        assert_eq!(receipt["artifacts"].as_array().unwrap().len(), 1);
        assert_eq!(receipt["usage"]["input_tokens"], 500);
        assert_eq!(receipt["usage"]["output_tokens"], 200);
    }

    #[test]
    fn protocol_state_full_happy_path() {
        let mut state = ProtocolState::new();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);

        state.advance(&hello_frame("test")).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingRun);

        let run = Frame::Run {
            id: "r1".into(),
            work_order: json!({}),
        };
        state.advance(&run).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Streaming);

        let event = event_frame("r1", event_text_delta("hi"));
        state.advance(&event).unwrap();
        assert_eq!(state.events_seen(), 1);

        let final_frame = Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({}),
        };
        state.advance(&final_frame).unwrap();
        assert_eq!(state.phase(), ProtocolPhase::Completed);
        assert!(state.is_terminal());
    }

    #[test]
    fn protocol_state_rejects_event_before_hello() {
        let mut state = ProtocolState::new();
        let event = event_frame("r1", event_text_delta("hi"));
        assert!(state.advance(&event).is_err());
        assert_eq!(state.phase(), ProtocolPhase::Faulted);
    }

    #[test]
    fn protocol_state_reset_clears_state() {
        let mut state = ProtocolState::new();
        state.advance(&hello_frame("b")).unwrap();
        state.reset();
        assert_eq!(state.phase(), ProtocolPhase::AwaitingHello);
        assert!(state.run_id().is_none());
    }

    #[test]
    fn validate_frame_catches_empty_contract_version() {
        let frame = Frame::Hello {
            contract_version: "".into(),
            backend: json!({"id": "b"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let validation = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!validation.valid);
        assert!(
            validation
                .issues
                .iter()
                .any(|i| i.contains("contract_version"))
        );
    }

    #[test]
    fn validate_frame_catches_wrong_contract_prefix() {
        let frame = Frame::Hello {
            contract_version: "wrong/v1".into(),
            backend: json!({"id": "b"}),
            capabilities: json!({}),
            mode: Value::Null,
        };
        let v = validate_frame(&frame, 16 * 1024 * 1024);
        assert!(!v.valid);
    }

    #[test]
    fn validate_frame_catches_oversized() {
        let frame = Frame::Ping { seq: 1 };
        let v = validate_frame(&frame, 5);
        assert!(!v.valid);
        assert!(v.issues.iter().any(|i| i.contains("exceeds")));
    }

    #[test]
    fn event_builders_produce_timestamped_values() {
        let builders: Vec<Value> = vec![
            event_text_delta("d"),
            event_text_message("m"),
            event_error("e"),
            event_warning("w"),
            event_run_started("s"),
            event_run_completed("c"),
            event_file_changed("f.rs", "modified"),
            event_command_executed("cargo build", Some(0), None),
            event_tool_call("read_file", None, json!({})),
            event_tool_result("read_file", None, json!("ok"), false),
        ];
        for ev in &builders {
            assert!(ev.get("ts").is_some(), "missing ts in {ev}");
            assert!(ev.get("type").is_some(), "missing type in {ev}");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Edge cases: empty object, null, arrays, deeply nested
// ═══════════════════════════════════════════════════════════════════════
mod edge_cases {
    use super::*;

    #[test]
    fn event_with_null_payload() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: Value::Null,
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert!(event.is_null()),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_with_empty_object_payload() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({}),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert_eq!(event, json!({})),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_with_array_payload() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!([1, "two", null, true, [3, 4]]),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => {
                let arr = event.as_array().unwrap();
                assert_eq!(arr.len(), 5);
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn deeply_nested_json_value() {
        let mut v = json!("leaf");
        for _ in 0..50 {
            v = json!({"nested": v});
        }
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: v.clone(),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => {
                // Walk down 50 levels
                let mut cursor = &event;
                for _ in 0..50 {
                    cursor = &cursor["nested"];
                }
                assert_eq!(cursor, &json!("leaf"));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_with_boolean_payload() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!(true),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert_eq!(event, json!(true)),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_with_numeric_payload() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!(3.14159),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => {
                assert!((event.as_f64().unwrap() - 3.14159).abs() < 1e-10);
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_with_string_payload() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!("just a string"),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => assert_eq!(event, "just a string"),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn fatal_with_no_ref_id() {
        let frame = Frame::Fatal {
            ref_id: None,
            error: "no ref".into(),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Fatal { ref_id, .. } => assert!(ref_id.is_none()),
            _ => panic!("expected Fatal"),
        }
    }

    #[test]
    fn empty_string_ref_id() {
        let frame = Frame::Event {
            ref_id: "".into(),
            event: json!(null),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { ref_id, .. } => assert_eq!(ref_id, ""),
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn try_event_extracts_typed_value() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"key": "value"}),
        };
        let (rid, val): (String, std::collections::HashMap<String, String>) =
            frame.try_event().unwrap();
        assert_eq!(rid, "r1");
        assert_eq!(val["key"], "value");
    }

    #[test]
    fn try_event_on_non_event_returns_error() {
        let frame = Frame::Ping { seq: 1 };
        let result: Result<(String, Value), _> = frame.try_event();
        assert!(result.is_err());
    }

    #[test]
    fn try_final_extracts_typed_receipt() {
        let frame = Frame::Final {
            ref_id: "r1".into(),
            receipt: json!({"outcome": "complete"}),
        };
        let (rid, val): (String, std::collections::HashMap<String, String>) =
            frame.try_final().unwrap();
        assert_eq!(rid, "r1");
        assert_eq!(val["outcome"], "complete");
    }

    #[test]
    fn try_final_on_non_final_returns_error() {
        let frame = Frame::Ping { seq: 1 };
        let result: Result<(String, Value), _> = frame.try_final();
        assert!(result.is_err());
    }

    #[test]
    fn special_json_characters_in_string_values() {
        let frame = Frame::Event {
            ref_id: "r1".into(),
            event: json!({"text": "line1\nline2\ttab \"quoted\" \\backslash"}),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Event { event, .. } => {
                let text = event["text"].as_str().unwrap();
                assert!(text.contains('\n'));
                assert!(text.contains('\t'));
                assert!(text.contains('"'));
                assert!(text.contains('\\'));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn ping_pong_seq_zero() {
        let frame = Frame::Ping { seq: 0 };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Ping { seq } => assert_eq!(seq, 0),
            _ => panic!("expected Ping"),
        }
    }

    #[test]
    fn ping_pong_seq_max_u64() {
        let frame = Frame::Ping { seq: u64::MAX };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Ping { seq } => assert_eq!(seq, u64::MAX),
            _ => panic!("expected Ping"),
        }
    }

    #[test]
    fn work_order_with_mixed_value_types() {
        let frame = Frame::Run {
            id: "r1".into(),
            work_order: json!({
                "task": "build",
                "count": 42,
                "active": true,
                "config": null,
                "tags": ["rust", "test"],
                "meta": {"nested": {"deep": true}}
            }),
        };
        let encoded = JsonlCodec::encode(&frame).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match decoded {
            Frame::Run { work_order, .. } => {
                assert_eq!(work_order["task"], "build");
                assert_eq!(work_order["count"], 42);
                assert_eq!(work_order["active"], true);
                assert!(work_order["config"].is_null());
                assert_eq!(work_order["tags"].as_array().unwrap().len(), 2);
                assert_eq!(work_order["meta"]["nested"]["deep"], true);
            }
            _ => panic!("expected Run"),
        }
    }
}
