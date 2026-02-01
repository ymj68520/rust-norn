//! Compressed network message wrapper
//!
//! This module provides a wrapper for compressed network messages,
//! allowing efficient transmission of large data structures.

use serde::{Serialize, Deserialize};
use anyhow::Result;

use crate::compression::{CompressionAlgorithm, Compressor, CompressionConfig};

/// Compressed message wrapper
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompressedMessage {
    /// Original (uncompressed) size in bytes
    #[serde(default)]
    pub original_size: usize,

    /// Compression algorithm used
    pub algorithm: CompressionAlgorithm,

    /// Compressed data
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

impl CompressedMessage {
    /// Create a new compressed message
    pub fn new(
        data: Vec<u8>,
        algorithm: CompressionAlgorithm,
        original_size: usize,
    ) -> Self {
        Self {
            original_size,
            algorithm,
            data,
        }
    }

    /// Compress raw data into a CompressedMessage
    pub fn compress(data: &[u8], config: &CompressionConfig) -> Result<Self> {
        let compressor = Compressor::with_config(*config);
        let compressed = compressor.compress(data)?;

        // Determine actual algorithm used (might be None if data was too small)
        let algorithm = if config.adaptive && compressed.len() == data.len() {
            CompressionAlgorithm::None
        } else {
            config.algorithm
        };

        Ok(Self {
            original_size: data.len(),
            algorithm,
            data: compressed,
        })
    }

    /// Decompress the message back to raw data
    pub fn decompress(&self) -> Result<Vec<u8>> {
        let compressor = Compressor::new();
        compressor.decompress(&self.data, self.algorithm)
    }

    /// Get compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if self.original_size == 0 {
            0.0
        } else {
            (self.data.len() as f64 / self.original_size as f64) * 100.0
        }
    }

    /// Check if compression was beneficial
    pub fn is_compressed(&self) -> bool {
        self.algorithm != CompressionAlgorithm::None && self.data.len() < self.original_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compressed_message_creation() {
        let msg = CompressedMessage::new(
            vec![1, 2, 3],
            CompressionAlgorithm::Zstd,
            3,
        );

        assert_eq!(msg.original_size, 3);
        assert_eq!(msg.algorithm, CompressionAlgorithm::Zstd);
        assert_eq!(msg.data, vec![1, 2, 3]);
    }

    #[test]
    fn test_compression_ratio() {
        let msg = CompressedMessage {
            original_size: 1000,
            algorithm: CompressionAlgorithm::Zstd,
            data: vec![0u8; 200],
        };

        let ratio = msg.compression_ratio();
        assert_eq!(ratio, 20.0); // 20% of original size
    }

    #[test]
    fn test_is_compressed() {
        let compressed = CompressedMessage {
            original_size: 1000,
            algorithm: CompressionAlgorithm::Zstd,
            data: vec![0u8; 200],
        };
        assert!(compressed.is_compressed());

        let not_compressed = CompressedMessage {
            original_size: 100,
            algorithm: CompressionAlgorithm::Zstd,
            data: vec![0u8; 150], // Larger than original
        };
        assert!(!not_compressed.is_compressed());

        let no_compression = CompressedMessage {
            original_size: 1000,
            algorithm: CompressionAlgorithm::None,
            data: vec![0u8; 200],
        };
        assert!(!no_compression.is_compressed());
    }
}
