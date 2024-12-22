use anyhow::{Context, Result};
use std::path::PathBuf;

pub mod quic;

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
    pub const CHALLENGE_REQUEST_BUFFER_LENGTH: usize = 256;
    pub const TCP_QUIC_FORWARDING_BUFFER_SIZE: usize = 1024 * 10;
    pub const TCP_DIRECT_FORWARDING_BUFFER_SIZE: usize = 1024 * 10;
}

pub type ByteCount = u64;
