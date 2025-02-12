use anyhow::{Context, Result};
use tokio::io::{copy_bidirectional, AsyncRead, AsyncWrite};
use tracing::info;

pub async fn forward_bidirectional<A, B, T>(
    a: &mut A,
    b: &mut B,
    stream_id_a: T,
    stream_id_b: T,
) -> Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
    T: std::fmt::Display,
{
    let (n1, n2) = copy_bidirectional(a, b)
        .await
        .context("Bidirectional copy failed")?;
    info!(
        "Stream id (A={} B={}): forwarded {} bytes in A->B direction and {} bytes in B->A direction",
        stream_id_a, stream_id_b, n1, n2
    );
    Ok(())
}
