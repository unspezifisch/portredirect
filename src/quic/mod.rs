pub mod client;
pub mod server;

#[allow(unused)]
pub const ALPN_QUIC_PORTREDIRECT: &[&[u8]] = &[b"pr-1"]; // QUIC ALPN field: port redirect protocol v1
