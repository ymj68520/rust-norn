//! Logging Configuration
//!
//! This module provides structured logging configuration for the Norn node.

use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

/// Logging configuration
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Log level
    pub level: String,

    /// Log format: "json" or "pretty"
    pub format: LogFormat,

    /// Log outputs
    pub outputs: Vec<LogOutput>,

    /// Log file path (required if outputs contains File)
    pub file_path: Option<String>,

    /// Maximum log file size in MB
    pub max_file_size: u64,

    /// Maximum number of log files to keep
    pub max_files: usize,

    /// Compress rotated log files
    pub compress: bool,
}

/// Log format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Pretty,
}

impl AsRef<str> for LogFormat {
    fn as_ref(&self) -> &str {
        match self {
            LogFormat::Json => "json",
            LogFormat::Pretty => "pretty",
        }
    }
}

impl std::str::FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(LogFormat::Json),
            "pretty" => Ok(LogFormat::Pretty),
            _ => Err(format!("Unknown log format: {}", s)),
        }
    }
}

/// Log output destination
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogOutput {
    Stdout,
    File,
}

impl AsRef<str> for LogOutput {
    fn as_ref(&self) -> &str {
        match self {
            LogOutput::Stdout => "stdout",
            LogOutput::File => "file",
        }
    }
}

impl LoggingConfig {
    /// Create a new logging config with defaults
    pub fn new() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Json,
            outputs: vec![LogOutput::Stdout],
            file_path: None,
            max_file_size: 100,
            max_files: 10,
            compress: true,
        }
    }

    /// Initialize the logging system
    ///
    /// Returns a WorkerGuard that must be kept alive for the duration of the program
    pub fn init(&self) -> Result<Option<WorkerGuard>, anyhow::Error> {
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(&self.level));

        let mut guards = Vec::new();

        match self.format {
            LogFormat::Json => {
                // JSON format for production
                if self.outputs.contains(&LogOutput::Stdout) {
                    let subscriber = tracing_subscriber::registry()
                        .with(env_filter.clone())
                        .with(fmt::layer().json());
                    tracing::subscriber::set_global_default(subscriber)
                        .map_err(|e| anyhow::anyhow!("Failed to set subscriber: {}", e))?;
                }

                if self.outputs.contains(&LogOutput::File) {
                    let file_path = self.file_path.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("File output requires file_path"))?;

                    let file_appender = tracing_appender::rolling::daily(
                        file_path,
                        "norn.log",
                    );
                    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
                    guards.push(guard);

                    let subscriber = tracing_subscriber::registry()
                        .with(env_filter.clone())
                        .with(fmt::layer().json().with_writer(non_blocking));
                    tracing::subscriber::set_global_default(subscriber)
                        .map_err(|e| anyhow::anyhow!("Failed to set subscriber: {}", e))?;
                }
            }
            LogFormat::Pretty => {
                // Pretty format for development
                if self.outputs.contains(&LogOutput::Stdout) {
                    let subscriber = tracing_subscriber::registry()
                        .with(env_filter.clone())
                        .with(fmt::layer().pretty().with_span_events(FmtSpan::CLOSE));
                    tracing::subscriber::set_global_default(subscriber)
                        .map_err(|e| anyhow::anyhow!("Failed to set subscriber: {}", e))?;
                }

                if self.outputs.contains(&LogOutput::File) {
                    let file_path = self.file_path.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("File output requires file_path"))?;

                    let file_appender = tracing_appender::rolling::daily(
                        file_path,
                        "norn.log",
                    );
                    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
                    guards.push(guard);

                    let subscriber = tracing_subscriber::registry()
                        .with(env_filter.clone())
                        .with(fmt::layer().pretty().with_writer(non_blocking).with_span_events(FmtSpan::CLOSE));
                    tracing::subscriber::set_global_default(subscriber)
                        .map_err(|e| anyhow::anyhow!("Failed to set subscriber: {}", e))?;
                }
            }
        }

        // Return the first guard (if any) - must be kept alive
        Ok(guards.into_iter().next())
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_logging_config_creation() {
        let config = LoggingConfig::new();
        assert_eq!(config.level, "info");
        assert_eq!(config.outputs.len(), 1);
    }

    #[test]
    fn test_log_format_parsing() {
        assert_eq!(LogFormat::from_str("json").unwrap(), LogFormat::Json);
        assert_eq!(LogFormat::from_str("pretty").unwrap(), LogFormat::Pretty);
        assert!(LogFormat::from_str("invalid").is_err());
    }
}
