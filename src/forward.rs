// PortRedirector-RS
//
// License: GPL-3.0-only

use std::error::Error;

use anyhow::{Context, Result};
use tokio::io::{copy_bidirectional, AsyncRead, AsyncWrite};
use tracing::{info, warn};

/// Checks whether the error represents a graceful shutdown (error 0).
fn is_graceful_shutdown<T: Error>(err: &T) -> bool {
    err.to_string().contains("error 0")
}

pub async fn forward_bidirectional<A, B, Ax, Bx>(
    a: &mut A,
    b: &mut B,
    id_a: Ax,
    id_b: Bx,
) -> Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
    Ax: std::fmt::Display,
    Bx: std::fmt::Display,
{
    let result = copy_bidirectional(a, b).await;

    match result {
        Ok((n1, n2)) => {
            info!(
                "Stream id (A={} B={}): forwarded {} bytes in A->B direction and {} bytes in B->A direction",
                id_a, id_b, n1, n2
            );
            Ok(())
        }
        Err(err) => {
            if is_graceful_shutdown(&err) {
                warn!(
                    "Stream id (A={} B={}): bidirectional copy finished gracefully (error 0)",
                    id_a, id_b
                );
                Ok(())
            } else {
                Err(err).context("Bidirectional copy failed")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    // A simple in-memory test stream that has a read buffer and collects written data.
    #[derive(Debug)]
    struct TestStream {
        // Data that will be "read" from this stream.
        read_data: Vec<u8>,
        // Data that has been "written" to this stream.
        write_data: Vec<u8>,
        // Current read position.
        pos: usize,
    }

    impl TestStream {
        fn new(initial_data: &[u8]) -> Self {
            Self {
                read_data: initial_data.to_vec(),
                write_data: Vec::new(),
                pos: 0,
            }
        }
    }

    impl AsyncRead for TestStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            let remaining = &self.read_data[self.pos..];
            if remaining.is_empty() {
                // Signal EOF by not filling any new data.
                return Poll::Ready(Ok(()));
            }
            let to_copy = std::cmp::min(remaining.len(), buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            self.pos += to_copy;
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for TestStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            self.write_data.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    // A stream that fails immediately when read or written.
    struct FailingStream;

    impl AsyncRead for FailingStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "read failure",
            )))
        }
    }

    impl AsyncWrite for FailingStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "write failure",
            )))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn test_forward_bidirectional_success() -> Result<()> {
        // Stream A will "send" the bytes in "hello" and expect to receive data from B.
        let mut stream_a = TestStream::new(b"hello");
        // Stream B will "send" the bytes in "world" and expect to receive data from A.
        let mut stream_b = TestStream::new(b"world");

        // Run the forwarding function.
        forward_bidirectional(&mut stream_a, &mut stream_b, "A", "B").await?;

        // After bidirectional copy, stream_a should have received stream_b's data, and vice versa.
        assert_eq!(stream_a.write_data, b"world");
        assert_eq!(stream_b.write_data, b"hello");

        Ok(())
    }

    #[tokio::test]
    async fn test_forward_bidirectional_failure() {
        let mut normal_stream = TestStream::new(b"data");
        let mut failing_stream = FailingStream;

        // One of the streams will immediately fail; our function should return an error with the proper context.
        let result =
            forward_bidirectional(&mut failing_stream, &mut normal_stream, "fail", "normal").await;
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.err().unwrap());
        assert!(
            err_msg.contains("Bidirectional copy failed"),
            "Error message did not contain expected context, got: {}",
            err_msg
        );
    }

    // A stream that simulates a graceful shutdown error (i.e. returns an error whose
    // message contains "error 0").
    struct GracefulStream;

    impl AsyncRead for GracefulStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "sending stopped by peer: error 0",
            )))
        }
    }

    impl AsyncWrite for GracefulStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "sending stopped by peer: error 0",
            )))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), std::io::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn test_forward_bidirectional_graceful_shutdown() -> Result<()> {
        // Use a normal TestStream for one side.
        let mut normal_stream = TestStream::new(b"normal");
        // Use the graceful stream for the other side.
        let mut graceful_stream = GracefulStream;

        // When one side produces an error containing "error 0", our forward_bidirectional
        // function should treat it as a graceful shutdown and return Ok(()).
        forward_bidirectional(
            &mut normal_stream,
            &mut graceful_stream,
            "normal",
            "graceful",
        )
        .await?;

        Ok(())
    }
}
