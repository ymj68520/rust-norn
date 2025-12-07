use norn_common::types::{Hash, Transaction};
use sha2::{Digest, Sha256};

#[derive(Clone)]
struct Node {
    data: Vec<u8>,
    #[allow(dead_code)] 
    left: Option<Box<Node>>,
    #[allow(dead_code)]
    right: Option<Box<Node>>,
}

/// Computes the SHA256 hash of left + right data.
fn merge_hash(left: &Node, right: &Node) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(&left.data);
    hasher.update(&right.data);
    hasher.finalize().to_vec()
}

/// Builds the Merkle Tree and returns the root hash.
/// Replicates the exact logic from go-norn/core/merkle.go
/// Including the specific behavior for odd nodes and single node.
pub fn build_merkle_tree(txs: &[Transaction]) -> Hash {
    let length = txs.len();
    if length == 0 {
        return Hash::default(); // [0; 32]
    }

    // Initialize leaf nodes
    let mut nodes: Vec<Node> = txs
        .iter()
        .map(|tx| Node {
            data: tx.body.hash.0.to_vec(),
            left: None,
            right: None,
        })
        .collect();

    // Go: height := int(math.Log2(float64(length))) + 1
    let height = (length as f64).log2() as usize + 1;

    for _ in 0..height {
        let mut level_len = nodes.len() / 2;
        if nodes.len() % 2 != 0 {
            level_len += 1;
        }
        
        let mut next_level = Vec::with_capacity(level_len);

        for i in (0..nodes.len()).step_by(2) {
            if i + 1 >= nodes.len() {
                // Pair with null node
                let null_node = Node {
                    data: vec![],
                    left: None,
                    right: None,
                };
                let left = &nodes[i];
                let merged_data = merge_hash(left, &null_node);
                
                next_level.push(Node {
                    data: merged_data,
                    left: Some(Box::new(left.clone())),
                    right: Some(Box::new(null_node)),
                });
            } else {
                let left = &nodes[i];
                let right = &nodes[i + 1];
                let merged_data = merge_hash(left, right);

                next_level.push(Node {
                    data: merged_data,
                    left: Some(Box::new(left.clone())),
                    right: Some(Box::new(right.clone())),
                });
            }
        }
        nodes = next_level;
        
        // Go: if len(nodes) == 1 { break }
        if nodes.len() == 1 {
            break;
        }
    }

    // Convert result Vec<u8> to Hash
    let mut root_hash = Hash::default();
    if let Some(root) = nodes.first() {
        if root.data.len() == 32 {
            root_hash.0.copy_from_slice(&root.data);
        } else {
            // If data < 32 (e.g. initial hash), copy what we can or pad?
            // SHA256 output is always 32. 
            // The only case it's not is if we had 0 transactions but we return early.
            // Or if initial tx hash wasn't 32 bytes (but Hash type enforces it).
        }
    }
    root_hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::{Transaction, TransactionBody, Hash};

    fn create_tx(byte: u8) -> Transaction {
        let mut tx = Transaction::default();
        tx.body.hash.0[0] = byte;
        tx
    }

    #[test]
    fn test_merkle_empty() {
        let txs = vec![];
        let root = build_merkle_tree(&txs);
        assert_eq!(root, Hash::default());
    }

    #[test]
    fn test_merkle_single() {
        // Go logic: Single node is paired with null node
        let tx = create_tx(1);
        let txs = vec![tx];
        let root = build_merkle_tree(&txs);
        
        // Expected: Hash(Hash(tx) + Hash(null))
        
        let mut hasher = Sha256::new();
        hasher.update(&txs[0].body.hash.0);
        hasher.update(&[]);
        let expected_bytes = hasher.finalize();
        
        assert_eq!(root.0.as_slice(), expected_bytes.as_slice());
    }

    #[test]
    fn test_merkle_pair() {
        let tx1 = create_tx(1);
        let tx2 = create_tx(2);
        let txs = vec![tx1.clone(), tx2.clone()];
        let root = build_merkle_tree(&txs);

        let mut hasher = Sha256::new();
        hasher.update(&tx1.body.hash.0);
        hasher.update(&tx2.body.hash.0);
        let expected_bytes = hasher.finalize();

        assert_eq!(root.0.as_slice(), expected_bytes.as_slice());
    }
    
    #[test]
    fn test_merkle_three() {
        let tx1 = create_tx(1);
        let tx2 = create_tx(2);
        let tx3 = create_tx(3);
        let txs = vec![tx1.clone(), tx2.clone(), tx3.clone()];
        let root = build_merkle_tree(&txs);

        // Level 1:
        // N1 = Hash(T1 + T2)
        let mut h1 = Sha256::new();
        h1.update(&tx1.body.hash.0);
        h1.update(&tx2.body.hash.0);
        let n1_data = h1.finalize();
        
        // N2 = Hash(T3 + Null)
        let mut h2 = Sha256::new();
        h2.update(&tx3.body.hash.0);
        h2.update(&[]);
        let n2_data = h2.finalize();
        
        // Root = Hash(N1 + N2)
        let mut hr = Sha256::new();
        hr.update(&n1_data);
        hr.update(&n2_data);
        let expected_root = hr.finalize();
        
        assert_eq!(root.0.as_slice(), expected_root.as_slice());
    }
}
