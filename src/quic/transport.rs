// PortRedirector-RS QUIC Connection Module
//
// License: GPL-3.0-only

use quinn::{RecvStream, SendStream};
use std::fmt::{self, Display, Formatter};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub struct GenericQuinnStream {
    pub send: SendStream,
    pub recv: RecvStream,
}

impl GenericQuinnStream {
    pub fn new(send: SendStream, recv: RecvStream) -> Self {
        Self { send, recv }
    }
}

impl Display for GenericQuinnStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "QuinnAuthStream(send: {:?}, recv: {:?})", self.send, self.recv)
    }
}

impl AsyncWrite for GenericQuinnStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match Pin::new(&mut self.send).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => Poll::Ready(Ok(n)),
            Poll::Ready(Err(e)) => {
                // Convert Quinn's WriteError into std::io::Error
                Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)))
            }
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
        // We call it here and map the error accordingly.
        match self.send.finish() {
            Ok(()) => Poll::Ready(Ok(())),
            Err(e) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
        }
    }
}

impl AsyncRead for GenericQuinnStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}
