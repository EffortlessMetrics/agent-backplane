// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stdio pipe setup utilities for sidecar processes.
//!
//! [`StdioPipes`] bundles the stdin/stdout/stderr handles of a child
//! process and provides convenience methods for wrapping them in buffered
//! readers suitable for JSONL protocol communication.

use tokio::io::{AsyncBufRead, AsyncWrite, BufReader, BufWriter};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};

/// Default buffer size for stdio pipe readers/writers (64 KiB).
pub const DEFAULT_BUF_SIZE: usize = 64 * 1024;

/// Bundles the stdio handles of a sidecar child process.
///
/// After spawning a process, extract the raw handles and wrap them here
/// for protocol communication.
///
/// # Examples
///
/// ```no_run
/// use abp_sidecar_utils::stdio::StdioPipes;
///
/// # async fn example() {
/// // Typically obtained from SidecarProcess::take_stdin() etc.
/// // let pipes = StdioPipes::new(stdin, stdout, stderr);
/// // let (writer, reader, err_reader) = pipes.into_buffered();
/// # }
/// ```
pub struct StdioPipes {
    /// Stdin handle for writing to the sidecar.
    pub stdin: ChildStdin,
    /// Stdout handle for reading protocol output.
    pub stdout: ChildStdout,
    /// Stderr handle for capturing diagnostic output.
    pub stderr: ChildStderr,
}

impl StdioPipes {
    /// Wrap raw child process handles.
    pub fn new(stdin: ChildStdin, stdout: ChildStdout, stderr: ChildStderr) -> Self {
        Self {
            stdin,
            stdout,
            stderr,
        }
    }

    /// Convert into buffered I/O handles with the default buffer size.
    ///
    /// Returns `(writer, reader, stderr_reader)`.
    pub fn into_buffered(
        self,
    ) -> (
        BufWriter<ChildStdin>,
        BufReader<ChildStdout>,
        BufReader<ChildStderr>,
    ) {
        self.into_buffered_with_capacity(DEFAULT_BUF_SIZE)
    }

    /// Convert into buffered I/O handles with a custom buffer capacity.
    pub fn into_buffered_with_capacity(
        self,
        capacity: usize,
    ) -> (
        BufWriter<ChildStdin>,
        BufReader<ChildStdout>,
        BufReader<ChildStderr>,
    ) {
        (
            BufWriter::with_capacity(capacity, self.stdin),
            BufReader::with_capacity(capacity, self.stdout),
            BufReader::with_capacity(capacity, self.stderr),
        )
    }
}

impl std::fmt::Debug for StdioPipes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioPipes").finish_non_exhaustive()
    }
}

/// Wrapper providing a protocol-oriented view of the stdio pipes.
///
/// Separates the write channel (stdin) from the read channel (stdout)
/// so they can be used concurrently in separate tasks.
pub struct ProtocolPipes<W, R> {
    /// Writer for sending envelopes to the sidecar (stdin).
    pub writer: W,
    /// Reader for receiving envelopes from the sidecar (stdout).
    pub reader: R,
}

impl<W: AsyncWrite + Unpin, R: AsyncBufRead + Unpin> ProtocolPipes<W, R> {
    /// Create protocol pipes from a writer and reader.
    pub fn new(writer: W, reader: R) -> Self {
        Self { writer, reader }
    }
}

impl<W: std::fmt::Debug, R: std::fmt::Debug> std::fmt::Debug for ProtocolPipes<W, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtocolPipes").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn default_buf_size_is_reasonable() {
        const { assert!(DEFAULT_BUF_SIZE >= 4096) };
        const { assert!(DEFAULT_BUF_SIZE <= 1024 * 1024) };
    }

    #[test]
    fn protocol_pipes_debug() {
        let (r, _w) = tokio::io::duplex(64);
        let (r2, _w2) = tokio::io::duplex(64);
        let reader = BufReader::new(r);
        let pipes = ProtocolPipes::new(r2, reader);
        let debug = format!("{pipes:?}");
        assert!(debug.contains("ProtocolPipes"));
    }
}
