use prometheus::{register_int_counter, IntCounter};
use prometheus::{Encoder, TextEncoder};
use std::net::SocketAddr;
use warp::Filter;

use crate::metrics_helper::MetricsCounter;

pub async fn start_metrics_server(addr: impl Into<SocketAddr>) {
    let metrics_route = warp::path("metrics").map(|| {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        warp::reply::with_header(buffer, "Content-Type", encoder.format_type())
    });
    warp::serve(metrics_route).run(addr).await;
}

// Somewhere during initialization:
lazy_static::lazy_static! {
    pub static ref CONNECTIONS_ACCEPTED: IntCounter =
        register_int_counter!("connections_accepted_total", "Total number of accepted connections").unwrap();
    pub static ref BYTES_TRANSMITTED_A: IntCounter =
        register_int_counter!("bytes_transmitted_a_total", "Total number of bytes transmitted").unwrap();
    pub static ref BYTES_TRANSMITTED_B: IntCounter =
        register_int_counter!("bytes_transmitted_b_total", "Total number of bytes transmitted").unwrap();
    pub static ref TCP_FORWARDING_ERRORS: IntCounter =
        register_int_counter!("tcp_forwarding_errors_total", "Total number of TCP forwarding errors encountered").unwrap();
    pub static ref KEEPALIVE_ERRORS: IntCounter =
        register_int_counter!("keepalive_errors_total", "Total number of errors of the keepalive loop encountered").unwrap();
    pub static ref SERVER_CONNECTIONS_OPENED_TOTAL: IntCounter =
        register_int_counter!("server_connections_opened_total", "Total number of times the client has connected to the QUIC/PRRS server").unwrap();
    pub static ref SERVER_CONNECTIONS_GRACEFULLY_CLOSED_TOTAL: IntCounter =
        register_int_counter!("server_connections_gracefully_closed_total", "Total number of times the server connection was closed cleanly").unwrap();
}

impl MetricsCounter for prometheus::IntCounter {
    fn inc_by(&self, amount: u64) {
        self.inc_by(amount);
    }
}
