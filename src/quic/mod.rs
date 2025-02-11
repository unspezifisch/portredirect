// PortRedirector-RS QUIC Connection Module
//
// License: GPL-3.0-only

pub mod client;
pub mod server;
pub mod transport;

pub const ALPN_QUIC_PORTREDIRECT: &[&[u8]] = &[b"pr-1"]; // QUIC ALPN field: port redirect protocol v1
