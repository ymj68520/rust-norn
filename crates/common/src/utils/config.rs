use anyhow::{Context, Result};
use ::config::{Config, File};
use serde::de::DeserializeOwned;
use std::path::Path;

/// Loads configuration from a file into a struct.
/// Supports TOML, YAML, JSON, etc. based on file extension.
pub fn load_config<T: DeserializeOwned, P: AsRef<Path>>(path: P) -> Result<T> {
    let path_str = path.as_ref().to_str().context("Invalid config path")?;
    
    let settings = Config::builder()
        .add_source(File::with_name(path_str))
        .build()
        .context("Failed to build configuration")?;
        
    settings.try_deserialize::<T>().context("Failed to deserialize configuration")
}
