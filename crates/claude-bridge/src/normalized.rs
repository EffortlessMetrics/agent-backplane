//! Normalized mode: maps raw Value events to typed `AgentEvent` and `Receipt`.
//!
//! This module is only available when the `normalized` feature is enabled,
//! which pulls in `abp-core`.

#[cfg(feature = "normalized")]
mod inner {
    use sidecar_kit::RawRun;
    use tokio::sync::{mpsc, oneshot};
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_stream::StreamExt;

    use abp_core::{AgentEvent, Receipt};

    use crate::{BridgeError, ClaudeBridgeConfig, raw::RunOptions};

    /// A normalized run with typed events and receipt.
    pub struct NormalizedRun {
        /// Stream of typed `AgentEvent`s.
        pub events: ReceiverStream<AgentEvent>,

        /// The final typed `Receipt`.
        pub result: oneshot::Receiver<Result<Receipt, BridgeError>>,

        /// Handle to the background task.
        pub wait: tokio::task::JoinHandle<()>,

        /// Cancel token.
        pub cancel: sidecar_kit::CancelToken,
    }

    impl NormalizedRun {
        /// Create a NormalizedRun by wrapping a RawRun and mapping Value → typed.
        pub(crate) fn from_raw(raw: RawRun) -> Self {
            let (raw_events, raw_result, raw_wait, cancel) = raw.into_parts();

            let (ev_tx, ev_rx) = mpsc::channel::<AgentEvent>(256);
            let (result_tx, result_rx) = oneshot::channel::<Result<Receipt, BridgeError>>();

            let wait = tokio::spawn(async move {
                let mut events = raw_events;
                while let Some(value) = events.next().await {
                    match serde_json::from_value::<AgentEvent>(value) {
                        Ok(event) => {
                            if ev_tx.send(event).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(target: "claude_bridge", "failed to deserialize event: {e}");
                        }
                    }
                }

                // Wait for the receipt
                match raw_result.await {
                    Ok(Ok(value)) => match serde_json::from_value::<Receipt>(value) {
                        Ok(receipt) => {
                            let _ = result_tx.send(Ok(receipt));
                        }
                        Err(e) => {
                            let _ = result_tx.send(Err(BridgeError::Run(format!(
                                "failed to deserialize receipt: {e}"
                            ))));
                        }
                    },
                    Ok(Err(e)) => {
                        let _ = result_tx.send(Err(BridgeError::Sidecar(e)));
                    }
                    Err(_) => {
                        let _ = result_tx.send(Err(BridgeError::Run(
                            "receipt channel closed".into(),
                        )));
                    }
                }

                let _ = raw_wait.await;
            });

            Self {
                events: ReceiverStream::new(ev_rx),
                result: result_rx,
                wait,
                cancel,
            }
        }
    }

    /// Run in normalized mode: task string → typed AgentEvent stream + typed Receipt.
    pub async fn run_normalized(
        config: &ClaudeBridgeConfig,
        task: &str,
        opts: RunOptions,
    ) -> Result<NormalizedRun, BridgeError> {
        let raw = crate::raw::run_mapped_raw(config, task, opts).await?;
        Ok(NormalizedRun::from_raw(raw))
    }
}

#[cfg(feature = "normalized")]
pub use inner::*;
