use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing_subscriber::{
    fmt,
    EnvFilter,
};

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Whether to write logs to file
    #[serde(default = "default_file_logging")]
    pub file_logging: bool,

    /// Log file directory
    #[serde(default = "default_log_dir")]
    pub log_dir: PathBuf,

    /// Log file name
    #[serde(default = "default_log_file")]
    pub log_file: String,

    /// Whether to log to console
    #[serde(default = "default_console_logging")]
    pub console_logging: bool,

    /// Log format (json, pretty, compact)
    #[serde(default = "default_log_format")]
    pub format: String,

    /// Whether to include timestamps
    #[serde(default = "default_include_timestamps")]
    pub include_timestamps: bool,

    /// Whether to include target/module
    #[serde(default = "default_include_target")]
    pub include_target: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_logging: default_file_logging(),
            log_dir: default_log_dir(),
            log_file: default_log_file(),
            console_logging: default_console_logging(),
            format: default_log_format(),
            include_timestamps: default_include_timestamps(),
            include_target: default_include_target(),
        }
    }
}

// Default values
fn default_log_level() -> String { "info".to_string() }
fn default_file_logging() -> bool { true }
fn default_log_dir() -> PathBuf { PathBuf::from("logs") }
fn default_log_file() -> String { "norn.log".to_string() }
fn default_console_logging() -> bool { true }
fn default_log_format() -> String { "pretty".to_string() }
fn default_include_timestamps() -> bool { true }
fn default_include_target() -> bool { true }

/// Log format types
#[derive(Debug, Clone, PartialEq)]
pub enum LogFormat {
    Json,
    Pretty,
    Compact,
}

impl From<&str> for LogFormat {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => LogFormat::Json,
            "compact" => LogFormat::Compact,
            _ => LogFormat::Pretty,
        }
    }
}

/// Initialize logging system
pub fn init_logging(config: &LoggingConfig) -> Result<(), Box<dyn std::error::Error>> {
    // Create log directory if it doesn't exist
    if config.file_logging {
        std::fs::create_dir_all(&config.log_dir)?;
    }

    // Build environment filter
    let env_filter = build_env_filter(config)?;

    // Create subscriber with console logging only for now
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(config.include_target);

    if config.format == "json" {
        subscriber.json().init();
    } else if config.format == "compact" {
        subscriber.compact().init();
    } else {
        subscriber.pretty().init();
    }

    tracing::info!("Logging system initialized with level: {}", config.level);
    Ok(())
}

/// Build environment filter from configuration
fn build_env_filter(config: &LoggingConfig) -> Result<EnvFilter, Box<dyn std::error::Error>> {
    let mut filter_string = config.level.clone();

    // Add RUST_LOG environment variable if present
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        if !rust_log.is_empty() {
            filter_string.push(',');
            filter_string.push_str(&rust_log);
        }
    }

    Ok(EnvFilter::try_new(filter_string)?)
}

/// Initialize logging for testing
pub fn init_test_logging() {
    let subscriber = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set test logging subscriber");
}

/// Get current log level from environment
pub fn get_log_level() -> String {
    std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string())
}

/// Set log level dynamically
pub fn set_log_level(level: &str) {
    std::env::set_var("RUST_LOG", level);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_logging_config_defaults() {
        let config = LoggingConfig::default();
        assert_eq!(config.level, "info");
        assert_eq!(config.format, "pretty");
        assert!(config.console_logging);
        assert!(config.file_logging);
    }

    #[test]
    fn test_log_format_conversion() {
        assert_eq!(LogFormat::from("json"), LogFormat::Json);
        assert_eq!(LogFormat::from("JSON"), LogFormat::Json);
        assert_eq!(LogFormat::from("compact"), LogFormat::Compact);
        assert_eq!(LogFormat::from("pretty"), LogFormat::Pretty);
        assert_eq!(LogFormat::from("invalid"), LogFormat::Pretty); // default
    }

    #[test]
    fn test_init_logging() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let config = LoggingConfig {
            level: "debug".to_string(),
            file_logging: false, // Disable file logging for test
            console_logging: false, // Disable console logging for test
            format: "json".to_string(),
            ..Default::default()
        };

        // This should not panic
        let result = init_logging(&config);
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_env_filter_building() {
        let config = LoggingConfig::default();
        let filter = build_env_filter(&config).unwrap();
        // We can't easily test filter content directly, but we can ensure it doesn't panic
        assert!(filter.to_string().contains("info"));
    }
}