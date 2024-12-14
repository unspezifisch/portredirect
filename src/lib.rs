use std::path::PathBuf;
use anyhow::{Result, Context};

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
