// PortRedirector-RS Server
//
// License: GPL-3.0-only

#[derive(Debug)]
pub struct QuinnWorkerBundle {
    pub quic_recv: quinn::RecvStream,
    pub quic_send: quinn::SendStream,
}
