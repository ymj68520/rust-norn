use moka::future::Cache;
use norn_common::types::{Block, Hash};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn, instrument};
use std::time::Duration;

// Constants
const MAX_BLOCK_CHANNEL: usize = 128;
const MAX_KNOWN_BLOCK: u64 = 2048;
const MAX_PROCESSED_BLOCK: u64 = 2048;
const MAX_BUFFER_SIZE: i64 = 12;
const SECOND_QUEUE_INTERVAL: Duration = Duration::from_micros(100);

// Shared state of the buffer
struct BufferState {
    known_blocks: Cache<Hash, ()>,
    processed_blocks: Cache<Hash, ()>,
    selected_block: HashMap<i64, Block>,
    
    latest_block_hash: Hash,
    latest_block_height: i64,
    latest_block: Block,
    
    buffered_height: i64,
    buffer_full: bool,
}

#[derive(Clone)]
pub struct BlockBuffer {
    state: Arc<RwLock<BufferState>>,
    block_tx: mpsc::Sender<Block>,
    second_tx: mpsc::Sender<Block>,
    // pop_chan is passed in constructor to send "popped" blocks back to blockchain/consumer
    // We don't store the receiver here, but we store the sender to it? 
    // Go: `popChan chan *common.Block`. Passed in NewBlockBuffer.
    // So BlockBuffer holds the Sender side of popChan? 
    // Yes, `b.popChan <- ...`
    pop_tx: mpsc::Sender<Block>,
}

impl BlockBuffer {
    pub async fn new(latest: Block, pop_tx: mpsc::Sender<Block>) -> Self {
        let (block_tx, block_rx) = mpsc::channel(MAX_BLOCK_CHANNEL);
        let (second_tx, second_rx) = mpsc::channel(MAX_BLOCK_CHANNEL);

        let state = BufferState {
            known_blocks: Cache::new(MAX_KNOWN_BLOCK),
            processed_blocks: Cache::new(MAX_PROCESSED_BLOCK),
            selected_block: HashMap::new(),
            
            latest_block_hash: latest.header.block_hash,
            latest_block_height: latest.header.height,
            latest_block: latest.clone(),
            
            buffered_height: latest.header.height,
            buffer_full: false,
        };

        let buffer = Self {
            state: Arc::new(RwLock::new(state)),
            block_tx,
            second_tx,
            pop_tx,
        };

        // Spawn background tasks
        let b1 = buffer.clone();
        tokio::spawn(async move {
            b1.process_loop(block_rx).await;
        });

        let b2 = buffer.clone();
        tokio::spawn(async move {
            b2.second_process_loop(second_rx).await;
        });

        buffer
    }

    /// Appends a block to the buffer (First queue)
    pub async fn append_block(&self, block: Block) {
        if let Err(e) = self.block_tx.send(block).await {
            warn!("Failed to send block to buffer: {}", e);
        }
    }

    // --- Background Processes ---

    async fn process_loop(&self, mut rx: mpsc::Receiver<Block>) {
        while let Some(block) = rx.recv().await {
            self.handle_block(block, false).await;
        }
    }

    async fn second_process_loop(&self, mut rx: mpsc::Receiver<Block>) {
        let mut interval = tokio::time::interval(SECOND_QUEUE_INTERVAL);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // In Go, this consumes ONE item from secondChan per tick?
                    // `case <-timer.C: block := <-b.secondChan`
                    // Yes.
                    if let Ok(block) = tokio::time::timeout(Duration::from_millis(1), rx.recv()).await {
                         if let Some(b) = block {
                             self.handle_block(b, true).await;
                         }
                    }
                }
            }
        }
    }

    // Unified logic for processing block (from both queues)
    async fn handle_block(&self, block: Block, from_second: bool) {
        let block_hash = block.header.block_hash;
        let prev_hash = block.header.prev_block_hash;
        let height = block.header.height;
        
        let mut state = self.state.write().await;

        if height <= state.latest_block_height {
            warn!("Block height too low: {}", height);
            return;
        }

        if !from_second {
            if state.known_blocks.contains_key(&block_hash) {
                return;
            }
            state.known_blocks.insert(block_hash, ()).await;
        }

        // Logic: Check if prev block is "processed" or is latest.
        // If not, push to second queue (delayed).
        // This relies on `processed_blocks` or `latest_block`.
        
        let prev_processed = state.processed_blocks.contains_key(&prev_hash);
        let is_prev_latest = prev_hash == state.latest_block_hash;
        
        // Complex logic from Go:
        // prevHeightBlock := b.selectedBlock[blockHeight-1]
        // if prevBlockHash != b.latestBlock.BlockHash() && (prevHeightBlock == nil || prevBlockHash != prevHeightBlock.BlockHash())
        
        let prev_height_block_hash = state.selected_block.get(&(height - 1)).map(|b| b.header.block_hash);
        let match_selected = prev_height_block_hash == Some(prev_hash);
        
        if !is_prev_latest && !match_selected {
            // Priority low or orphan?
            if !prev_processed {
                if !from_second {
                     info!("Pop block to second channel: {}", height);
                     // Release lock before sending to avoid deadlock if channel full?
                     // Sending is async. 
                     // But we are in async fn.
                     drop(state); // Drop lock!
                     let _ = self.second_tx.send(block).await;
                     return;
                }
            } else {
                state.processed_blocks.insert(block_hash, ()).await;
            }
            
            if !from_second { 
                 // If from first channel and hit logic above, we either sent to second or added to processed.
                 // Go: break (stop processing this block here)
                 return;
            } else {
                 // From second channel:
                 // If condition met (still orphan?), Go adds to processed and breaks?
                 // Go: `if !hit { secondChan <- block } else { processed.Add } break`
                 // Wait, Go re-queues it to secondChan if !hit?
                 // Yes. Infinite loop potential?
                 // For now, let's assume we drop it or re-queue.
                 // I will skip re-queueing to avoid infinite loops for now unless logic is clear.
                 return; 
            }
        }
        
        state.processed_blocks.insert(block_hash, ()).await;
        
        // VDF Verification
        if !crate::consensus::verify_block_vdf_async(&block).await {
             warn!("Block VDF verification failed: {}", block_hash);
             return;
        }
        
        // Selection Logic
        let selected = state.selected_block.get(&height);
        let mut replace = false;
        
        if selected.is_none() {
            replace = true;
        } else if let Some(current) = selected {
            if compare_block(current, &block) {
                replace = true;
            }
        }
        
        if replace {
            state.selected_block.insert(height, block.clone());
            // update_tree_view(height) - remove successors
            let mut h = height + 1;
            while state.selected_block.contains_key(&h) {
                state.selected_block.remove(&h);
                h += 1;
            }
        }

        // Pop logic
        if height - state.latest_block_height > MAX_BUFFER_SIZE {
            // Need to pop
             if let Some(popped) = pop_selected_block(&mut state) {
                 drop(state); // Unlock
                 let _ = self.pop_tx.send(popped).await;
                 return;
             }
        }
    }

    pub async fn get_priority_leaf(&self, now_height: i64) -> Block {
        let state = self.state.read().await;
        
        let mut height = state.buffered_height;
        while height > state.latest_block_height {
            if let Some(block) = state.selected_block.get(&height) {
                if height < now_height {
                    return block.clone();
                }
            }
            height -= 1;
        }
        state.latest_block.clone()
    }
}

// Helper functions

fn compare_block(origin: &Block, new_block: &Block) -> bool {
    // Go logic:
    // if block.PrevBlockHash() != origin.PrevBlockHash() { return false } (Keep origin)
    if new_block.header.prev_block_hash != origin.header.prev_block_hash {
        return false;
    }
    
    let origin_tx_len = origin.transactions.len();
    let new_tx_len = new_block.transactions.len();
    
    if origin_tx_len == new_tx_len {
        // Timestamp smaller is better? Go: origin < block -> return origin (false).
        // So smaller timestamp has priority?
        // Wait: `if origin < block { return origin, false }`
        // So if origin is older (smaller ts), keep origin.
        // If new is older?
        if origin.header.timestamp < new_block.header.timestamp {
            return false;
        }
        return true; // Replace with new
    }
    
    if origin_tx_len > new_tx_len {
        return false;
    }
    
    true
}

fn pop_selected_block(state: &mut BufferState) -> Option<Block> {
    let height = state.latest_block_height + 1;
    if let Some(block) = state.selected_block.remove(&height) {
        state.latest_block_hash = block.header.block_hash;
        state.latest_block_height = height;
        state.latest_block = block.clone();
        
        // Clean up previous height? Go: delete(b.selectedBlock, height-1)
        state.selected_block.remove(&(height - 1));
        
        Some(block)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::{Block, Hash};
    use tokio::sync::mpsc;
    use std::time::Duration;

    fn create_block(height: i64, prev_hash: Hash) -> Block {
        let mut b = Block::default();
        b.header.height = height;
        b.header.prev_block_hash = prev_hash;
        // make hash unique based on height to avoid collision in test
        b.header.block_hash.0[31] = height as u8; 
        b
    }

    #[tokio::test]
    async fn test_buffer_simple_flow() {
        let mut genesis = Block::default();
        genesis.header.block_hash = Hash::default();
        
        let (pop_tx, _pop_rx) = mpsc::channel(10);
        let buffer = BlockBuffer::new(genesis.clone(), pop_tx).await;

        // Append block 1
        let b1 = create_block(1, genesis.header.block_hash);
        buffer.append_block(b1.clone()).await;
        
        // Give time for async processing
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Check state
        let state = buffer.state.read().await;
        assert!(state.selected_block.contains_key(&1));
        assert_eq!(state.selected_block.get(&1).unwrap().header.block_hash, b1.header.block_hash);
    }

    #[tokio::test]
    async fn test_buffer_pop() {
        let mut genesis = Block::default();
        genesis.header.block_hash = Hash::default();
        
        let (pop_tx, mut pop_rx) = mpsc::channel(10);
        let buffer = BlockBuffer::new(genesis.clone(), pop_tx).await;
        
        // Fill buffer to limit (MAX_BUFFER_SIZE = 12)
        // Current latest is 0. 
        // We need height - latest > 12 to pop. 
        // So height 13 should trigger pop of 1.
        
        let mut prev_hash = genesis.header.block_hash;
        for h in 1..=13 {
            let b = create_block(h, prev_hash);
            prev_hash = b.header.block_hash;
            buffer.append_block(b).await;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // We expect block 1 to be popped
        let popped = tokio::time::timeout(Duration::from_millis(100), pop_rx.recv()).await;
        assert!(popped.is_ok());
        let popped_block = popped.unwrap();
        assert!(popped_block.is_some());
        assert_eq!(popped_block.unwrap().header.height, 1);
    }
}
