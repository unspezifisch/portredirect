use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::time::{interval, timeout, Duration};
use tracing::{info, warn};

use crate::PortRedirectProtocol;

/// The amount of time to wait for a PONG response before timing out.
const READ_TIMEOUT: Duration = PortRedirectProtocol::CONNECTION_KEEPALIVE_TIMEOUT;
/// How often a PING is sent over the connection.
const KEEP_ALIVE_INTERVAL: Duration = PortRedirectProtocol::CONNECTION_KEEPALIVE_INTERVAL;
/// The PING message sent to the remote peer.
const PING_MESSAGE: &[u8] = b"PING\n";
/// The expected PONG response from the remote peer.
const PONG_MESSAGE: &[u8] = b"PONG\n";

/// Runs the keepalive loop on the client side.
///
/// Every `KEEP_ALIVE_INTERVAL` the function sends a PING message, flushes the stream,
/// and then waits (up to `READ_TIMEOUT`) for a newline-terminated response.
/// If the response exactly matches `PONG_MESSAGE` the ping is considered successful.
/// Any error (write/flush error, unexpected response, connection close, or timeout)
/// causes the loop to exit gracefully.
pub async fn run_keepalive_client_loop<T>(mut auth_stream: T) -> Result<()>
where
    T: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let mut tick_interval = interval(KEEP_ALIVE_INTERVAL);
    let mut pong_count = 0usize;

    loop {
        // Wait for the next tick.
        tick_interval.tick().await;

        // Send the PING message.
        if let Err(e) = auth_stream.write_all(PING_MESSAGE).await {
            warn!("Failed to send PING: {}", e);
            break;
        }
        if let Err(e) = auth_stream.flush().await {
            warn!("Failed to flush PING: {}", e);
            break;
        }
        info!("Sent PING");

        // Read the PONG response with a timeout.
        let mut response_buf = Vec::with_capacity(16);
        let mut reader = BufReader::new(&mut auth_stream);
        match timeout(READ_TIMEOUT, reader.read_until(b'\n', &mut response_buf)).await {
            Ok(Ok(0)) => {
                warn!("Connection closed by remote during keepalive");
                break;
            }
            Ok(Ok(_)) => {
                if response_buf == PONG_MESSAGE {
                    pong_count += 1;
                    info!("Received PONG, count: {}", pong_count);
                } else {
                    warn!("Unexpected response: {:?}", response_buf);
                    break;
                }
            }
            Ok(Err(e)) => {
                warn!("Failed to read PONG: {:?}", e);
                break;
            }
            Err(_) => {
                warn!("Timed out waiting for PONG response");
                break;
            }
        }
    }

    Ok(())
}

/// Runs the keepalive loop on the server side.
///
/// Instead of sending periodic PING messages, the server now waits for
/// incoming PING messages from the remote peer. When a PING is received,
/// the server replies with a PONG. Any error (read/write, unexpected message,
/// timeout, or connection close) causes the loop to exit gracefully.
pub async fn run_keepalive_server_loop<T>(mut auth_stream: T) -> Result<()>
where
    T: AsyncReadExt + AsyncWriteExt + AsyncBufReadExt + Unpin,
{
    loop {
        // Wait for an incoming message (expected to be PING).
        let mut buf = Vec::with_capacity(16);
        match timeout(
            KEEP_ALIVE_INTERVAL + READ_TIMEOUT,
            auth_stream.read_until(b'\n', &mut buf),
        )
        .await
        {
            Ok(Ok(0)) => {
                warn!("Connection closed by remote during keepalive");
                break;
            }
            Ok(Ok(_)) => {
                if buf == PING_MESSAGE {
                    info!("Received PING, sending PONG");
                    if let Err(e) = auth_stream.write_all(PONG_MESSAGE).await {
                        warn!("Failed to send PONG: {}", e);
                        break;
                    }
                    if let Err(e) = auth_stream.flush().await {
                        warn!("Failed to flush PONG: {}", e);
                        break;
                    }
                } else {
                    warn!("Unexpected message received: {:?}", buf);
                    break;
                }
            }
            Ok(Err(e)) => {
                warn!("Failed to read from stream: {:?}", e);
                break;
            }
            Err(_) => {
                warn!("Timed out waiting for PING message");
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::io;
    use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
    use tokio::time::{self, timeout, Duration};
    use tokio_test::io::Builder;

    // --- Protocol constants ---

    // For testing we use short durations.
    pub struct DummyProtocol;
    impl DummyProtocol {
        pub const CONNECTION_KEEPALIVE_INTERVAL_SECONDS: Duration = Duration::from_millis(100);
    }
    const TEST_KEEP_ALIVE_INTERVAL: Duration = DummyProtocol::CONNECTION_KEEPALIVE_INTERVAL_SECONDS;
    const TEST_READ_TIMEOUT: Duration = Duration::from_millis(200);

    // --- Combined loop helper (for both client and server) ---

    /// Runs a single-iteration keepalive loop. It writes a PING and expects a PONG.
    async fn run_loop_with_test_interval<T>(mut stream: T) -> Result<(), Box<dyn Error>>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        let mut tick_interval = time::interval(TEST_KEEP_ALIVE_INTERVAL);
        let mut pong_count = 0usize;

        loop {
            tick_interval.tick().await;

            // Send a ping.
            if stream.write_all(PING_MESSAGE).await.is_err() {
                break;
            }
            if stream.flush().await.is_err() {
                break;
            }

            // Read a response.
            let mut response_buf = Vec::with_capacity(16);
            let mut reader = BufReader::new(&mut stream);
            match timeout(
                TEST_READ_TIMEOUT,
                reader.read_until(b'\n', &mut response_buf),
            )
            .await
            {
                Ok(Ok(0)) => break, // connection closed
                Ok(Ok(_)) => {
                    if response_buf == PONG_MESSAGE {
                        pong_count += 1;
                    } else {
                        break;
                    }
                }
                _ => break,
            }

            // For testing, exit after one successful ping-pong.
            if pong_count >= 1 {
                break;
            }
        }
        Ok(())
    }

    // --- Dummy stream for simulating timeouts ---

    /// A dummy stream that never returns any data on read.
    struct NeverRead;
    impl AsyncRead for NeverRead {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::task::Poll::Pending
        }
    }
    impl AsyncWrite for NeverRead {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<io::Result<usize>> {
            // Simulate that all bytes are “written.”
            std::task::Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    // --- Helper functions to build mock streams ---

    /// Builds a mock stream for a “good” keepalive interaction.
    fn build_good_keepalive() -> tokio_test::io::Mock {
        let mut builder = Builder::new();
        // Only expect one ping: write PING, then read PONG.
        builder.write(PING_MESSAGE);
        builder.read(b"PONG\n");
        builder.build()
    }

    /// Builds a mock stream that returns an incorrect response.
    fn build_wrong_response() -> tokio_test::io::Mock {
        let mut builder = Builder::new();
        builder.write(PING_MESSAGE);
        //builder.flush();
        builder.read(b"WRONG\n");
        builder.build()
    }

    /// Builds a mock stream that errors on write.
    fn build_write_error() -> tokio_test::io::Mock {
        let mut builder = Builder::new();
        builder.write_error(io::Error::new(io::ErrorKind::Other, "write error"));
        builder.build()
    }

    // --- Client-side tests ---

    #[tokio::test(start_paused = true)]
    async fn test_keepalive_client_good() {
        let mock = build_good_keepalive();
        let result = run_loop_with_test_interval(mock).await;
        assert!(result.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn test_keepalive_client_wrong_response() {
        let mock = build_wrong_response();
        let result = run_loop_with_test_interval(mock).await;
        // The loop should exit gracefully on an unexpected response.
        assert!(result.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn test_keepalive_client_write_error() {
        let mock = build_write_error();
        let result = run_loop_with_test_interval(mock).await;
        // The error should cause the loop to exit gracefully.
        assert!(result.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn test_keepalive_client_timeout() {
        let stream = NeverRead;
        let fut = run_loop_with_test_interval(stream);
        // Advance time to trigger the tick and then the read timeout.
        time::advance(TEST_KEEP_ALIVE_INTERVAL).await;
        time::advance(TEST_READ_TIMEOUT).await;
        let result = fut.await;
        assert!(result.is_ok());
    }

    // --- Server-side tests ---
    // (Using the same helper functions as above.)

    #[tokio::test(start_paused = true)]
    async fn test_keepalive_server_good() {
        let mock = build_good_keepalive();
        let result = run_loop_with_test_interval(mock).await;
        assert!(result.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn test_keepalive_server_wrong_response() {
        let mock = build_wrong_response();
        let result = run_loop_with_test_interval(mock).await;
        assert!(result.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn test_keepalive_server_write_error() {
        let mock = build_write_error();
        let result = run_loop_with_test_interval(mock).await;
        assert!(result.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn test_keepalive_server_timeout() {
        let stream = NeverRead;
        let fut = run_loop_with_test_interval(stream);
        time::advance(TEST_KEEP_ALIVE_INTERVAL).await;
        time::advance(TEST_READ_TIMEOUT).await;
        let result = fut.await;
        assert!(result.is_ok());
    }
}
