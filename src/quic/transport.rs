// PortRedirector-RS QUIC Connection Module
//
// License: GPL-3.0-only
use quinn::{RecvStream, SendStream};
use std::cmp;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, ReadBuf};

/// A QUIC stream wrapper that implements AsyncRead, AsyncWrite, and AsyncBufRead
/// without relying on external crates for buffering.
pub struct GenericQuicStream {
    pub send: SendStream,
    pub recv: RecvStream,

    // Our own internal buffer: data available for consumption is stored in `buf[pos..]`.
    buf: Vec<u8>,
    pos: usize,
}

impl GenericQuicStream {
    pub fn new(send: SendStream, recv: RecvStream) -> Self {
        Self {
            send,
            recv,
            buf: Vec::with_capacity(8 * 1024),
            pos: 0,
        }
    }
}

impl Display for GenericQuicStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GenericQuicStream(send: {}, recv: {})",
            self.send.id(),
            self.recv.id()
        )
    }
}

impl AsyncWrite for GenericQuicStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match Pin::new(&mut self.send).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => Poll::Ready(Ok(n)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match Pin::new(&mut self.send).poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        // Quinn’s SendStream provides a synchronous finish() method.
        // Call it here and map the error accordingly.
        match self.send.finish() {
            Ok(()) => Poll::Ready(Ok(())),
            Err(e) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
        }
    }
}

impl AsyncRead for GenericQuicStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), io::Error>> {
        // Delegate directly to the underlying receive stream.
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncBufRead for GenericQuicStream {
    /// Fills the internal buffer (if empty) and returns a slice of the available data.
    fn poll_fill_buf<'a>(
        self: Pin<&'a mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<&'a [u8], io::Error>> {
        // Use get_mut() to work with the inner value.
        let this = self.get_mut();

        // If there is still unconsumed data, return it.
        if this.pos < this.buf.len() {
            return Poll::Ready(Ok(&this.buf[this.pos..]));
        }

        // Otherwise, try to fill the buffer. We'll use a temporary buffer.
        let mut temp = [0u8; 8 * 1024];
        let mut read_buf = ReadBuf::new(&mut temp);

        match Pin::new(&mut this.recv).poll_read(cx, &mut read_buf) {
            Poll::Ready(Ok(())) => {
                let n = read_buf.filled().len();
                if n == 0 {
                    // End-of-stream reached.
                    return Poll::Ready(Ok(&this.buf[..])); // likely an empty slice
                }
                // Replace the internal buffer with the newly read data.
                this.buf.clear();
                this.buf.extend_from_slice(read_buf.filled());
                this.pos = 0;
                Poll::Ready(Ok(&this.buf[..]))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    /// Advances the internal buffer by consuming `amt` bytes.
    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.get_mut();
        this.pos = cmp::min(this.pos + amt, this.buf.len());
        if this.pos == this.buf.len() {
            this.buf.clear();
            this.pos = 0;
        }
    }
}
