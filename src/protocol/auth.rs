// PortRedirector Protocol Module
//
// License: GPL-3.0-only

use crate::protocol::utils::ElapsedMinutes;
use crate::protocol::utils::TimeProvider;
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum allowed length for the challenge message.
const CHALLENGE_MAX_LEN: usize = 128;

/// Server-side authentication: send challenge, receive and verify the client’s response.
pub async fn server_authenticate<S>(
    stream: &mut S,
    psk: &[u8],
    time_provider: &impl TimeProvider,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // 1. Generate a challenge.
    let coarse_unix_time = time_provider.now().elapsed_minutes();
    let random_bytes = crate::protocol::utils::generate_random_bytes(32)?;
    let random_bytes_hex = hex::encode(random_bytes);

    let challenge = format!(
        "this-is-the-challenge-{}-at-{}-pr-v1",
        random_bytes_hex, coarse_unix_time
    );
    let request = format!("WHO THE HECK ARE YOU?\n{}\n", challenge);

    if request.len() > CHALLENGE_MAX_LEN {
        return Err(anyhow!("Challenge string exceeds maximum length"));
    }

    // 2. Send the challenge.
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;

    // 3. Read client response.
    let mut buf = vec![0u8; 65]; // 64 hex bytes + newline
    stream.read_exact(&mut buf).await?;
    let client_response = std::str::from_utf8(&buf)?.trim_end();

    // 4. Compute expected response.
    let mut hasher = Sha256::new();
    hasher.update(challenge.as_bytes());
    hasher.update(psk);
    let expected_response = hex::encode(hasher.finalize());

    // 5. Verify.
    if client_response == expected_response {
        stream.write_all(b"HAPPY\n").await?;
        stream.flush().await?;
        Ok(())
    } else {
        stream.write_all(b"BAD\n").await?;
        stream.flush().await?;
        Err(anyhow!("Authentication failed: response mismatch"))
    }
}

/// Client-side authentication: receive the challenge, compute the response, send it.
pub async fn client_authenticate<S>(stream: &mut S, psk: &[u8]) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // 1. Read the challenge message.
    let mut buf = vec![0u8; CHALLENGE_MAX_LEN];
    let n = stream.read(&mut buf).await?;
    let message = std::str::from_utf8(&buf[..n])?;
    let mut lines = message.lines();

    match lines.next() {
        Some("WHO THE HECK ARE YOU?") => {}
        _ => return Err(anyhow!("Unexpected authentication prompt")),
    }
    let challenge = lines
        .next()
        .ok_or_else(|| anyhow!("Missing challenge in authentication message"))?;

    // 2. Compute response.
    let mut hasher = Sha256::new();
    hasher.update(challenge.as_bytes());
    hasher.update(psk);
    let response = hex::encode(hasher.finalize()) + "\n";

    // 3. Send response.
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    // 4. Read final server reply.
    let mut reply = vec![0u8; 16];
    let n = stream.read(&mut reply).await?;
    let reply = std::str::from_utf8(&reply[..n])?;
    if reply.starts_with("HAPPY") {
        Ok(())
    } else {
        Err(anyhow!("Server rejected authentication: {}", reply.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::utils::SystemTimeProvider;
    use tokio::io::duplex;

    #[tokio::test]
    async fn test_authentication_protocol() -> Result<()> {
        let psk = b"test-secret";
        // Create an in‑memory duplex stream.
        let (mut client_side, mut server_side) = duplex(1024);

        // Run server and client auth concurrently.
        let server = tokio::spawn(async move {
            server_authenticate(&mut server_side, psk, &SystemTimeProvider).await
        });

        let client = tokio::spawn(async move { client_authenticate(&mut client_side, psk).await });

        server.await??;
        client.await??;
        Ok(())
    }
}
