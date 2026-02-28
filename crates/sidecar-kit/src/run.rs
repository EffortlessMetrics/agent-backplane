// SPDX-License-Identifier: MIT OR Apache-2.0
//! Value-based run: event streaming and receipt collection.

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tracing::warn;

use super::{SidecarError, cancel::CancelToken, frame::Frame, process::SidecarProcess};

/// An in-progress value-based sidecar run.
///
/// Provides a raw event stream, a receipt future, and a [`CancelToken`].
/// Dropping a `RawRun` automatically cancels the sidecar.
pub struct RawRun {
    /// Stream of raw event Values.
    pub events: ReceiverStream<Value>,

    /// The final receipt (as raw Value), or an error.
    pub result: oneshot::Receiver<Result<Value, SidecarError>>,

    /// Handle to the background event loop task.
    pub wait: tokio::task::JoinHandle<Result<(), SidecarError>>,

    /// Cancel token — signals the event loop to stop.
    pub cancel: CancelToken,
}

impl Drop for RawRun {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl RawRun {
    /// Consume the `RawRun` and return its constituent parts, disabling
    /// the automatic cancel-on-drop behavior.
    #[allow(clippy::type_complexity)]
    #[allow(unsafe_code)]
    pub fn into_parts(
        self,
    ) -> (
        ReceiverStream<Value>,
        oneshot::Receiver<Result<Value, SidecarError>>,
        tokio::task::JoinHandle<Result<(), SidecarError>>,
        CancelToken,
    ) {
        // SAFETY: We need to move fields out of a Drop type.
        // We use ManuallyDrop to prevent the destructor from running.
        let this = std::mem::ManuallyDrop::new(self);
        unsafe {
            let events = std::ptr::read(&this.events);
            let result = std::ptr::read(&this.result);
            let wait = std::ptr::read(&this.wait);
            let cancel = std::ptr::read(&this.cancel);
            (events, result, wait, cancel)
        }
    }

    pub(crate) fn start(mut process: SidecarProcess, run_id: String) -> Result<Self, SidecarError> {
        let (ev_tx, ev_rx) = mpsc::channel::<Value>(256);
        let (result_tx, result_rx) = oneshot::channel::<Result<Value, SidecarError>>();
        let cancel = CancelToken::new();
        let cancel_clone = cancel.clone();

        let wait = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_clone.cancelled() => {
                        // Send cancel frame
                        let cancel_frame = Frame::Cancel {
                            ref_id: run_id.clone(),
                            reason: Some("cancelled by caller".into()),
                        };
                        let _ = process.send(&cancel_frame).await;

                        // Grace period: drain for up to 5 seconds
                        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
                        loop {
                            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                            if remaining.is_zero() {
                                break;
                            }
                            match tokio::time::timeout(remaining, process.recv()).await {
                                Ok(Ok(Some(Frame::Final { .. }))) => break,
                                Ok(Ok(Some(Frame::Fatal { .. }))) => break,
                                Ok(Ok(None)) => break,
                                Ok(Err(_)) => break,
                                Err(_) => break, // timeout
                                _ => continue,
                            }
                        }

                        process.kill().await;
                        return Ok(());
                    }

                    frame = process.recv() => {
                        match frame {
                            Ok(Some(Frame::Event { ref_id, event })) => {
                                if ref_id != run_id {
                                    warn!(target: "sidecar_kit", "dropping event for other run_id={ref_id}");
                                    continue;
                                }
                                if ev_tx.send(event).await.is_err() {
                                    // Receiver dropped
                                    break;
                                }
                            }
                            Ok(Some(Frame::Final { ref_id, receipt })) => {
                                if ref_id != run_id {
                                    warn!(target: "sidecar_kit", "dropping final for other run_id={ref_id}");
                                    continue;
                                }
                                let _ = result_tx.send(Ok(receipt));
                                break;
                            }
                            Ok(Some(Frame::Fatal { ref_id, error })) => {
                                if let Some(ref rid) = ref_id
                                    && *rid != run_id
                                {
                                    warn!(target: "sidecar_kit", "dropping fatal for other run_id={rid}");
                                    continue;
                                }
                                let _ = result_tx.send(Err(SidecarError::Fatal(error)));
                                break;
                            }
                            Ok(Some(Frame::Hello { .. })) => {
                                // Ignore duplicate hello
                                continue;
                            }
                            Ok(Some(Frame::Pong { .. })) => {
                                continue;
                            }
                            Ok(Some(other)) => {
                                let _ = result_tx.send(Err(SidecarError::Protocol(
                                    format!("unexpected frame: {other:?}"),
                                )));
                                break;
                            }
                            Ok(None) => {
                                // EOF — sidecar exited without sending Final
                                let _ = result_tx.send(Err(SidecarError::Protocol(
                                    "sidecar exited without final frame".into(),
                                )));
                                break;
                            }
                            Err(e) => {
                                let _ = result_tx.send(Err(e));
                                break;
                            }
                        }
                    }
                }
            }

            process.kill().await;
            Ok(())
        });

        Ok(Self {
            events: ReceiverStream::new(ev_rx),
            result: result_rx,
            cancel,
            wait,
        })
    }
}
