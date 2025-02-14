// PortRedirector-RS QUIC Connection Module
//
// License: GPL-3.0-only

use quinn::{RecvStream, SendStream};
use std::fmt::{self, Display, Formatter};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, BufReader, ReadBuf};

/// A QUIC stream wrapper that implements AsyncRead and AsyncWrite.
/// Uses `BufReader` for efficient buffering on the receive side.
pub struct GenericQuicStream {
    pub send: SendStream,
    pub recv: BufReader<RecvStream>,
}

impl GenericQuicStream {
    /// Creates a new `GenericQuicStream` with a buffered receive stream.
    pub fn new(send: SendStream, recv: RecvStream) -> Self {
        Self {
            send,
            recv: BufReader::with_capacity(8 * 1024, recv), // TODO use constant; 8 KB buffer
        }
    }
}

impl Display for GenericQuicStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GenericQuicStream(send: {}, recv: {})",
            self.send.id(),
            self.recv.get_ref().id()
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
        // Delegate to the buffered receive stream.
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncBufRead for GenericQuicStream {
    fn poll_fill_buf<'a>(
        self: Pin<&'a mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<&'a [u8], io::Error>> {
        // Delegate to the buffered receive stream.
        Pin::new(&mut self.get_mut().recv).poll_fill_buf(cx)
    }

    fn consume(mut self: Pin<&mut Self>, amt: usize) {
        // Delegate to the buffered receive stream.
        Pin::new(&mut self.recv).consume(amt);
    }
}
