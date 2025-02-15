// PortRedirector-RS
//
// License: GPL-3.0-only

use anyhow::{Context, Result};
use std::{path::PathBuf, time::Duration};

pub mod app_data;
pub mod bi_stream;
pub mod client;
pub mod forward;
pub mod metrics_helper; 
pub mod protocol;
pub mod quic;
pub mod server;

/// Returns the path to the configuration directory, creating it if necessary.
pub fn get_config_dir() -> Result<PathBuf> {
    let mut config_dir =
        dirs::config_dir().context("Failed to find your platform's config directory")?;
    config_dir.push("portredirect");

    // Create the directory if it doesn't exist
    std::fs::create_dir_all(&config_dir).context("create config dir")?;

    Ok(config_dir)
}

pub struct PortRedirectProtocol;

impl PortRedirectProtocol {
    pub const CONNECTION_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
    pub const CONNECTION_KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(2);
    pub const CHALLENGE_REQUEST_BUFFER_LENGTH: usize = 256;

    pub const QUIC_STREAM_READ_BUFFER_SIZE: usize = 256 * 1024; // 256 KiB
}

pub type ByteCount = u64;
