use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{interval, Duration};
use tracing::{info, warn};

/// How often to send a PING message.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);

/// Runs the keepalive loop that periodically sends a PING and expects a PONG response.
pub async fn run_keepalive_loop<T>(mut auth_stream: T) -> Result<()>
where
    T: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let mut tick_interval = interval(KEEP_ALIVE_INTERVAL);
    let mut pong_count = 0usize;

    loop {
        // Wait until the next tick.
        tick_interval.tick().await;

        // Send a PING message.
        if let Err(e) = auth_stream.write_all(b"PING\n").await {
            warn!("Failed to send PING: {}", e);
            break;
        }
        info!("Sent PING");

        // Prepare a buffer to read the response.
        let mut buf = [0u8; 16];
        match auth_stream.read(&mut buf).await {
            Ok(n) if n > 0 => {
                if let Ok(text) = std::str::from_utf8(&buf[..n]) {
                    if text.starts_with("PONG") {
                        pong_count += 1;
                        info!("Received PONG, count: {}", pong_count);
                    } else {
                        warn!("Unexpected response: {}", text.trim());
                    }
                } else {
                    warn!("Received non-UTF8 data");
                }
            }
            Ok(_) => {
                warn!("Connection closed by remote during keepalive");
                break;
            }
            Err(e) => {
                warn!("Error reading from auth stream: {}", e);
                break;
            }
        }
    }

    Ok(())
}
