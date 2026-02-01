//! Write-Ahead Log (WAL) for database durability
//!
//! This module provides WAL functionality to ensure database durability
//! and enable crash recovery. All writes are first logged to the WAL
//! before being applied to the main database.

use norn_common::error::{NornError, Result};
use serde::{Serialize, Deserialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write, Seek, SeekFrom, BufWriter, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn, error};
use std::time::{SystemTime, UNIX_EPOCH};
use sha2::{Sha256, Digest};

/// WAL entry type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WALEntry {
    /// Account creation
    CreateAccount {
        address: [u8; 20],
        data: Vec<u8>,
    },

    /// Account update
    UpdateAccount {
        address: [u8; 20],
        data: Vec<u8>,
    },

    /// Account deletion
    DeleteAccount {
        address: [u8; 20],
    },

    /// Storage write
    WriteStorage {
        address: [u8; 20],
        key: Vec<u8>,
        value: Vec<u8>,
    },

    /// Storage deletion
    DeleteStorage {
        address: [u8; 20],
        key: Vec<u8>,
    },

    /// Checkpoint marker
    Checkpoint {
        block_number: u64,
        block_hash: [u8; 32],
    },

    /// Transaction begin
    TransactionBegin {
        id: u64,
    },

    /// Transaction commit
    TransactionCommit {
        id: u64,
    },

    /// Transaction rollback
    TransactionRollback {
        id: u64,
    },
}

/// WAL entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WALEntryWithMeta {
    /// Sequence number
    sequence: u64,

    /// Timestamp
    timestamp: u64,

    /// Entry type
    entry: WALEntry,

    /// Checksum
    checksum: [u8; 32],
}

impl WALEntryWithMeta {
    /// Create a new WAL entry with metadata
    fn new(sequence: u64, entry: WALEntry) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create checksum
        let checksum = Self::compute_checksum(&entry, sequence, timestamp);

        Self {
            sequence,
            timestamp,
            entry,
            checksum,
        }
    }

    /// Compute checksum for entry
    fn compute_checksum(entry: &WALEntry, sequence: u64, timestamp: u64) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Include sequence and timestamp in hash
        hasher.update(&sequence.to_le_bytes());
        hasher.update(&timestamp.to_le_bytes());

        // Hash entry data
        if let Ok(data) = bincode::serialize(entry) {
            hasher.update(&data);
        }

        let result = hasher.finalize();
        let mut checksum = [0u8; 32];
        checksum.copy_from_slice(&result);
        checksum
    }

    /// Verify checksum
    fn verify_checksum(&self) -> bool {
        let computed = Self::compute_checksum(&self.entry, self.sequence, self.timestamp);
        computed == self.checksum
    }
}

/// WAL configuration
#[derive(Debug, Clone)]
pub struct WALConfig {
    /// Maximum WAL file size in bytes (default: 100MB)
    pub max_file_size: usize,

    /// Maximum number of WAL files to keep (default: 5)
    pub max_files: usize,

    /// Sync after each write (default: true for durability)
    pub sync_on_write: bool,

    /// Create checkpoint after N entries (default: 1000)
    pub checkpoint_interval: u64,
}

impl Default for WALConfig {
    fn default() -> Self {
        Self {
            max_file_size: 100 * 1024 * 1024, // 100MB
            max_files: 5,
            sync_on_write: true,
            checkpoint_interval: 1000,
        }
    }
}

/// Write-Ahead Log
pub struct WAL {
    /// WAL directory
    wal_dir: PathBuf,

    /// Current WAL file
    current_file: Arc<Mutex<BufWriter<File>>>,

    /// Current WAL file path
    current_path: Arc<Mutex<PathBuf>>,

    /// Current file number
    file_number: Arc<Mutex<u64>>,

    /// Current sequence number
    sequence: Arc<Mutex<u64>>,

    /// Configuration
    config: WALConfig,

    /// Entries since last checkpoint
    entries_since_checkpoint: Arc<Mutex<u64>>,
}

impl WAL {
    /// Create or open a WAL
    pub fn new(wal_dir: impl AsRef<Path>, config: WALConfig) -> Result<Self> {
        let wal_dir = wal_dir.as_ref().to_path_buf();

        // Create WAL directory if it doesn't exist
        std::fs::create_dir_all(&wal_dir)
            .map_err(|e| NornError::Internal(format!("Failed to create WAL directory: {}", e)))?;

        // Find existing WAL files
        let existing_files = Self::list_wal_files(&wal_dir)?;

        let (file_number, sequence) = if existing_files.is_empty() {
            // New WAL
            info!("Creating new WAL at {:?}", wal_dir);
            (0, 0)
        } else {
            // Recover existing WAL
            let max_file = *existing_files.iter().max().unwrap();
            let sequence = Self::recover_sequence(&wal_dir, max_file)?;
            info!("Recovering WAL at {:?}, file={}, sequence={}", wal_dir, max_file, sequence);
            (max_file, sequence)
        };

        let current_path = wal_dir.join(format!("wal-{}.log", file_number));
        let current_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&current_path)
            .map_err(|e| NornError::Internal(format!("Failed to open WAL file: {}", e)))?;

        let wal = Self {
            wal_dir,
            current_file: Arc::new(Mutex::new(BufWriter::new(current_file))),
            current_path: Arc::new(Mutex::new(current_path)),
            file_number: Arc::new(Mutex::new(file_number)),
            sequence: Arc::new(Mutex::new(sequence)),
            config,
            entries_since_checkpoint: Arc::new(Mutex::new(0)),
        };

        // Sync existing file if recovering
        if file_number > 0 {
            wal.sync()?;
        }

        Ok(wal)
    }

    /// Write an entry to the WAL
    pub fn write(&self, entry: WALEntry) -> Result<u64> {
        // Get next sequence number
        let sequence = {
            let mut seq = self.sequence.lock()
                .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;
            *seq += 1;
            let seq = *seq;
            seq
        };

        // Create entry with metadata
        let entry_with_meta = WALEntryWithMeta::new(sequence, entry);

        // Verify checksum before writing
        if !entry_with_meta.verify_checksum() {
            return Err(NornError::Internal("WAL checksum verification failed".to_string()));
        }

        // Serialize entry
        let data = bincode::serialize(&entry_with_meta)
            .map_err(|e| NornError::Internal(format!("Failed to serialize WAL entry: {}", e)))?;

        // Write length prefix (4 bytes)
        let len = data.len() as u32;

        {
            let mut file = self.current_file.lock()
                .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;

            file.write_all(&len.to_le_bytes())
                .map_err(|e| NornError::Internal(format!("Failed to write WAL length: {}", e)))?;

            // Write entry data
            file.write_all(&data)
                .map_err(|e| NornError::Internal(format!("Failed to write WAL entry: {}", e)))?;

            // Flush if configured
            if self.config.sync_on_write {
                file.flush()
                    .map_err(|e| NornError::Internal(format!("Failed to flush WAL: {}", e)))?;
            }
        }

        debug!("WAL write: sequence={}, type={:?}", sequence, entry_with_meta.entry);

        // Check if we need to rotate file
        if self.should_rotate()? {
            self.rotate()?;
        }

        // Update checkpoint counter
        {
            let mut counter = self.entries_since_checkpoint.lock()
                .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;
            *counter += 1;
        }

        Ok(sequence)
    }

    /// Read all entries from WAL (for recovery)
    pub fn read_all(&self) -> Result<Vec<WALEntry>> {
        let mut entries = Vec::new();

        // Read from all WAL files in order
        let wal_files = Self::list_wal_files(&self.wal_dir)?;

        for &file_num in &wal_files {
            let path = self.wal_dir.join(format!("wal-{}.log", file_num));
            entries.extend(Self::read_file(&path)?);
        }

        info!("Read {} WAL entries for recovery", entries.len());
        Ok(entries)
    }

    /// Create a checkpoint marker
    pub fn checkpoint(&self, block_number: u64, block_hash: [u8; 32]) -> Result<()> {
        info!("Creating WAL checkpoint at block {}", block_number);

        let entry = WALEntry::Checkpoint {
            block_number,
            block_hash,
        };

        self.write(entry)?;

        // Reset checkpoint counter
        {
            let mut counter = self.entries_since_checkpoint.lock()
                .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;
            *counter = 0;
        }

        Ok(())
    }

    /// Sync the WAL to disk
    pub fn sync(&self) -> Result<()> {
        let mut file = self.current_file.lock()
            .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;

        file.flush()
            .map_err(|e| NornError::Internal(format!("Failed to sync WAL: {}", e)))?;

        file.get_ref().sync_all()
            .map_err(|e| NornError::Internal(format!("Failed to sync WAL file: {}", e)))?;

        Ok(())
    }

    /// Truncate WAL (remove old entries after checkpoint)
    pub fn truncate(&self) -> Result<()> {
        info!("Truncating WAL (keeping checkpoint files)");

        let wal_files = Self::list_wal_files(&self.wal_dir)?;

        // Keep only the last N files
        if wal_files.len() > self.config.max_files {
            let to_remove = wal_files.len() - self.config.max_files;
            let mut sorted_files: Vec<u64> = wal_files.iter().copied().collect();
            sorted_files.sort();

            for file_num in sorted_files.iter().take(to_remove) {
                let path = self.wal_dir.join(format!("wal-{}.log", file_num));
                if let Err(e) = std::fs::remove_file(&path) {
                    warn!("Failed to remove old WAL file {:?}: {}", path, e);
                } else {
                    debug!("Removed old WAL file {:?}", path);
                }
            }
        }

        Ok(())
    }

    /// Check if file rotation is needed
    fn should_rotate(&self) -> Result<bool> {
        let current_path = self.current_path.lock()
            .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;
        let metadata = std::fs::metadata(&*current_path)
            .map_err(|e| NornError::Internal(format!("Failed to get WAL file metadata: {}", e)))?;

        Ok(metadata.len() >= self.config.max_file_size as u64)
    }

    /// Rotate to a new WAL file
    fn rotate(&self) -> Result<()> {
        info!("Rotating WAL file");

        // Sync current file
        self.sync()?;

        // Increment file number
        {
            let mut file_number = self.file_number.lock()
                .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;
            *file_number += 1;
        }

        // Create new file
        let new_file_number = *self.file_number.lock()
            .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;
        let new_path = self.wal_dir.join(format!("wal-{}.log", new_file_number));

        let new_file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&new_path)
            .map_err(|e| NornError::Internal(format!("Failed to create new WAL file: {}", e)))?;

        // Replace current file and path
        {
            let mut file_guard = self.current_file.lock()
                .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;
            *file_guard = BufWriter::new(new_file);
        }

        {
            let mut current_path = self.current_path.lock()
                .map_err(|e| NornError::Internal(format!("WAL lock error: {}", e)))?;
            *current_path = new_path;
        }

        // Truncate old files
        self.truncate()?;

        Ok(())
    }

    /// List all WAL files in directory
    fn list_wal_files(wal_dir: &Path) -> Result<Vec<u64>> {
        let mut files = Vec::new();

        for entry in std::fs::read_dir(wal_dir)
            .map_err(|e| NornError::Internal(format!("Failed to read WAL directory: {}", e)))?
        {
            let entry = entry.map_err(|e| NornError::Internal(format!("Failed to read directory entry: {}", e)))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("log") {
                let file_stem = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");

                if file_stem.starts_with("wal-") {
                    if let Ok(num) = file_stem[4..].parse::<u64>() {
                        files.push(num);
                    }
                }
            }
        }

        Ok(files)
    }

    /// Recover sequence number from WAL file
    fn recover_sequence(wal_dir: &Path, file_num: u64) -> Result<u64> {
        let path = wal_dir.join(format!("wal-{}.log", file_num));
        let entries = Self::read_file(&path)?;

        // For now, estimate from entries count
        // A better approach would be to track this in a separate metadata file
        if entries.is_empty() {
            Ok(0)
        } else {
            Ok(entries.len() as u64)
        }
    }

    /// Read entries from a single WAL file
    fn read_file(path: &Path) -> Result<Vec<WALEntry>> {
        let file = File::open(path)
            .map_err(|e| NornError::Internal(format!("Failed to open WAL file {:?}: {}", path, e)))?;

        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();

        loop {
            // Read length prefix
            let mut len_bytes = [0u8; 4];
            if let Err(e) = reader.read_exact(&mut len_bytes) {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    break; // End of file
                }
                return Err(NornError::Internal(format!("Failed to read WAL entry length: {}", e)));
            }

            let len = u32::from_le_bytes(len_bytes) as usize;

            // Sanity check
            if len > 10_000_000 {
                return Err(NornError::Internal(format!("WAL entry too large: {} bytes", len)));
            }

            // Read entry data
            let mut data = vec![0u8; len];
            reader.read_exact(&mut data)
                .map_err(|e| NornError::Internal(format!("Failed to read WAL entry data: {}", e)))?;

            // Deserialize entry with metadata
            let entry_with_meta: WALEntryWithMeta = bincode::deserialize(&data)
                .map_err(|e| NornError::Internal(format!("Failed to deserialize WAL entry: {}", e)))?;

            // Verify checksum
            if !entry_with_meta.verify_checksum() {
                warn!("WAL entry checksum mismatch at sequence {}", entry_with_meta.sequence);
                continue;
            }

            entries.push(entry_with_meta.entry);
        }

        debug!("Read {} WAL entries from {:?}", entries.len(), path);
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_wal_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = WALConfig::default();

        let wal = WAL::new(temp_dir.path(), config).unwrap();

        assert_eq!(*wal.file_number.lock().unwrap(), 0);
        assert_eq!(*wal.sequence.lock().unwrap(), 0);
    }

    #[test]
    fn test_wal_write_read() {
        let temp_dir = TempDir::new().unwrap();
        let config = WALConfig::default();

        let wal = WAL::new(temp_dir.path(), config).unwrap();

        // Write entry
        let entry = WALEntry::CreateAccount {
            address: [1u8; 20],
            data: vec![2, 3, 4],
        };

        let sequence = wal.write(entry.clone()).unwrap();
        assert_eq!(sequence, 1);

        // Sync and read back
        wal.sync().unwrap();
        let entries = wal.read_all().unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], entry);
    }

    #[test]
    fn test_wal_checkpoint() {
        let temp_dir = TempDir::new().unwrap();
        let config = WALConfig::default();

        let wal = WAL::new(temp_dir.path(), config).unwrap();

        wal.checkpoint(100, [5u8; 32]).unwrap();

        let entries = wal.read_all().unwrap();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            WALEntry::Checkpoint { block_number, .. } => {
                assert_eq!(*block_number, 100);
            }
            _ => panic!("Expected checkpoint entry"),
        }
    }
}
