use anyhow::{Context, Result};
use tokio::io::{copy_bidirectional, AsyncRead, AsyncWrite};
use tracing::info;

pub async fn forward_bidirectional<A, B, Ax, Bx>(
    a: &mut A,
    b: &mut B,
    stream_id_a: Ax,
    stream_id_b: Bx,
) -> Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
    Ax: std::fmt::Display,
    Bx: std::fmt::Display,
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
