//! Network message compression
//!
//! This module provides compression utilities for network messages to reduce
//! bandwidth usage and improve synchronization performance.

use anyhow::{Result, anyhow};
use std::io::{Read, Write};
use tracing::{debug, trace};
use serde::{Serialize, Deserialize};

/// Compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// No compression
    None,

    /// Zstandard compression (zstd)
    /// - Fast compression and decompression
    /// - Good compression ratio
    /// - Configurable compression levels
    Zstd,

    /// Snappy compression
    /// - Very fast compression and decompression
    /// - Moderate compression ratio
    /// - Better for real-time scenarios
    Snappy,
}

impl Default for CompressionAlgorithm {
    fn default() -> Self {
        Self::Zstd
    }
}

/// Compression level for zstd
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionLevel {
    /// Fastest compression (level 1)
    Fast,
    /// Default compression (level 3)
    Default,
    /// Best compression (level 22)
    Best,
}

impl CompressionLevel {
    fn as_zstd_level(&self) -> i32 {
        match self {
            Self::Fast => 1,
            Self::Default => 3,
            Self::Best => 22,
        }
    }
}

impl Default for CompressionLevel {
    fn default() -> Self {
        Self::Default
    }
}

/// Compression configuration
#[derive(Debug, Clone, Copy)]
pub struct CompressionConfig {
    /// Compression algorithm
    pub algorithm: CompressionAlgorithm,

    /// Compression level (for zstd)
    pub level: CompressionLevel,

    /// Minimum size threshold (bytes)
    /// Messages smaller than this won't be compressed
    pub min_size: usize,

    /// Enable adaptive compression
    /// Automatically disable compression for small messages
    pub adaptive: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: CompressionAlgorithm::Zstd,
            level: CompressionLevel::Default,
            min_size: 256, // Don't compress messages < 256 bytes
            adaptive: true,
        }
    }
}

/// Compressor for network messages
pub struct Compressor {
    config: CompressionConfig,
}

impl Compressor {
    /// Create a new compressor with default config
    pub fn new() -> Self {
        Self::with_config(CompressionConfig::default())
    }

    /// Create a new compressor with custom config
    pub fn with_config(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Get the compression config
    pub fn config(&self) -> &CompressionConfig {
        &self.config
    }

    /// Compress data
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Skip compression if data is too small
        if self.config.adaptive && data.len() < self.config.min_size {
            trace!("Skipping compression for small message ({} bytes)", data.len());
            return Ok(data.to_vec());
        }

        match self.config.algorithm {
            CompressionAlgorithm::None => {
                Ok(data.to_vec())
            }
            CompressionAlgorithm::Zstd => {
                self.compress_zstd(data)
            }
            CompressionAlgorithm::Snappy => {
                self.compress_snappy(data)
            }
        }
    }

    /// Decompress data
    pub fn decompress(&self, data: &[u8], algorithm: CompressionAlgorithm) -> Result<Vec<u8>> {
        match algorithm {
            CompressionAlgorithm::None => {
                Ok(data.to_vec())
            }
            CompressionAlgorithm::Zstd => {
                self.decompress_zstd(data)
            }
            CompressionAlgorithm::Snappy => {
                self.decompress_snappy(data)
            }
        }
    }

    /// Compress using zstd
    fn compress_zstd(&self, data: &[u8]) -> Result<Vec<u8>> {
        let level = self.config.level.as_zstd_level();

        // Use zstd bulk compression API
        let compressed = zstd::bulk::compress(data, level)?;

        debug!(
            "Zstd compression: {} -> {} bytes (ratio: {:.2}%)",
            data.len(),
            compressed.len(),
            (compressed.len() as f64 / data.len() as f64) * 100.0
        );

        Ok(compressed)
    }

    /// Decompress zstd data
    fn decompress_zstd(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Use zstd bulk decompression API
        let decompressed = zstd::bulk::decompress(data, 10 * 1024 * 1024)?; // 10MB max output
        debug!("Zstd decompression: {} -> {} bytes", data.len(), decompressed.len());

        Ok(decompressed)
    }

    /// Compress using snappy
    fn compress_snappy(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut encoder = snap::raw::Encoder::new();
        let compressed = encoder.compress_vec(data)?;

        debug!(
            "Snappy compression: {} -> {} bytes (ratio: {:.2}%)",
            data.len(),
            compressed.len(),
            (compressed.len() as f64 / data.len() as f64) * 100.0
        );

        Ok(compressed)
    }

    /// Decompress snappy data
    fn decompress_snappy(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = snap::raw::Decoder::new();
        let decompressed = decoder.decompress_vec(data)?;

        debug!("Snappy decompression: {} -> {} bytes", data.len(), decompressed.len());

        Ok(decompressed)
    }
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_config_default() {
        let config = CompressionConfig::default();
        assert_eq!(config.algorithm, CompressionAlgorithm::Zstd);
        assert_eq!(config.min_size, 256);
        assert!(config.adaptive);
    }

    #[test]
    fn test_compressor_skip_small_messages() {
        let compressor = Compressor::new();
        let small_data = vec![1u8; 100];

        let compressed = compressor.compress(&small_data).unwrap();
        assert_eq!(compressed, small_data); // Should not compress small data
    }

    #[test]
    #[cfg(feature = "zstd")]
    fn test_zstd_compression() {
        let compressor = Compressor::with_config(CompressionConfig {
            algorithm: CompressionAlgorithm::Zstd,
            level: CompressionLevel::Fast,
            min_size: 0,
            adaptive: false,
        });

        let data = vec![42u8; 1000];
        let compressed = compressor.compress(&data).unwrap();

        // Compressed data should be smaller (or at least not much larger)
        assert!(compressed.len() < data.len() + 100);

        // Decompress and verify
        let decompressed = compressor.decompress(&compressed, CompressionAlgorithm::Zstd).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_no_compression() {
        let compressor = Compressor::with_config(CompressionConfig {
            algorithm: CompressionAlgorithm::None,
            ..Default::default()
        });

        let data = vec![1u8, 2u8, 3u8];
        let compressed = compressor.compress(&data).unwrap();

        assert_eq!(compressed, data);
    }
}
