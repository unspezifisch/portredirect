use std::{
    fmt,
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub struct BiStream<R, W> {
    pub read: R,
    pub write: W,
}

impl<R, W> BiStream<R, W> {
    pub fn new(read: R, write: W) -> Self {
        Self { read, write }
    }
}

// For AsyncRead, we require both R and W to be Unpin.
// Even though only R is used in poll_read, the whole BiStream must be Unpin
// for get_mut() to be available.
impl<R: AsyncRead + Unpin, W: Unpin> AsyncRead for BiStream<R, W> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // Since BiStream is Unpin (by our bounds), get_mut() is allowed.
        Pin::new(&mut self.get_mut().read).poll_read(cx, buf)
    }
}

// For AsyncWrite, we restrict R to Unpin and require W to be AsyncWrite + Unpin.
impl<R: Unpin, W: AsyncWrite + Unpin> AsyncWrite for BiStream<R, W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().write).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().write).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().write).poll_shutdown(cx)
    }
}

impl<R, W> fmt::Display for BiStream<R, W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BiStream {{ read_id: XX, write_id: XX }}",
        )
    }
}
