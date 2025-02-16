// PortRedirector-RS QUIC Connection Module
//
// License: GPL-3.0-only

use quinn::TransportConfig;

pub mod client;
pub mod server;

pub const ALPN_QUIC_PORTREDIRECT: &[&[u8]] = &[b"pr-1"]; // QUIC ALPN field: port redirect protocol v1

pub fn configure_transport_config(transport_config: &mut TransportConfig) {
    // QUIC connection advanced configuration
    transport_config.max_concurrent_uni_streams(1_u8.into()); // Not used at all by us, set lowest limit
    transport_config.max_concurrent_bidi_streams(0_u8.into()); // We manage our own connection limit
    transport_config.keep_alive_interval(Some(std::time::Duration::from_secs(25))); // 30s timeout is QUIC's default timeout
    transport_config.crypto_buffer_size(crate::PortRedirectProtocol::QUIC_CRYPTO_BUFFER_SIZE);
    transport_config.allow_spin(false); // We don't want to "sacrifice privacy" to more easily measure latency
}
