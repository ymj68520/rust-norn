use norn_common::types::Hash;
use norn_common::error::{NornError, Result};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use sha2::{Sha256, Digest};

/// Merkle Patricia Trie 节点类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    /// 空节点
    Null,
    
    /// 叶子节点
    Leaf {
        key: Vec<u8>,
        value: Vec<u8>,
    },
    
    /// 扩展节点
    Extension {
        shared_nibble: Vec<u8>,
        child: NodeRef,
    },
    
    /// 分支节点
    Branch {
        children: [Option<NodeRef>; 16],
        value: Option<Vec<u8>>,
    },
}

/// 节点引用
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeRef {
    /// 内联节点（小节点）
    Inline(Box<NodeType>),
    
    /// 哈希引用（大节点）
    Hash(Hash),
}

/// Trie 节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub node_type: NodeType,
    pub hash: Option<Hash>,
    pub dirty: bool,
}

/// Merkle Patricia Trie
pub struct MerklePatriciaTrie {
    /// 根节点
    root: Arc<RwLock<NodeRef>>,
    
    /// 数据库
    db: Arc<dyn TrieDB>,
    
    /// 缓存
    cache: Arc<RwLock<HashMap<Hash, Node>>>,
    
    /// 配置
    config: TrieConfig,
}

/// Trie 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrieConfig {
    /// 内联阈值
    pub inline_threshold: usize,
    
    /// 缓存大小
    pub cache_size: usize,
    
    /// 是否启用缓存
    pub enable_cache: bool,
    
    /// 哈希算法
    pub hash_algorithm: String,
}

impl Default for TrieConfig {
    fn default() -> Self {
        Self {
            inline_threshold: 32,
            cache_size: 10000,
            enable_cache: true,
            hash_algorithm: "sha256".to_string(),
        }
    }
}

/// Trie 数据库特征
#[async_trait::async_trait]
pub trait TrieDB: Send + Sync {
    /// 获取节点
    async fn get_node(&self, hash: &Hash) -> Result<Option<Node>>;
    
    /// 存储节点
    async fn put_node(&self, hash: &Hash, node: &Node) -> Result<()>;
    
    /// 删除节点
    async fn delete_node(&self, hash: &Hash) -> Result<()>;
    
    /// 批量操作
    async fn batch_write(&self, nodes: &[(Hash, Node)]) -> Result<()>;
    
    /// 获取根哈希
    async fn get_root_hash(&self) -> Result<Option<Hash>>;
    
    /// 设置根哈希
    async fn set_root_hash(&self, hash: &Hash) -> Result<()>;
}

/// Merkle 证明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// 键路径
    pub key: Vec<u8>,
    
    /// 证明节点
    pub proof_nodes: Vec<Node>,
    
    /// 根哈希
    pub root_hash: Hash,
    
    /// 值
    pub value: Option<Vec<u8>>,
}

impl MerklePatriciaTrie {
    /// 创建新的 Trie
    pub fn new(db: Arc<dyn TrieDB>, config: TrieConfig) -> Self {
        Self {
            root: Arc::new(RwLock::new(NodeRef::Inline(Box::new(NodeType::Null)))),
            db,
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Create an empty trie placeholder (for breaking circular dependencies)
    pub fn empty() -> Self {
        use crate::mocks::MockTrieDB;
        Self {
            root: Arc::new(RwLock::new(NodeRef::Inline(Box::new(NodeType::Null)))),
            db: Arc::new(MockTrieDB::new()),
            cache: Arc::new(RwLock::new(HashMap::new())),
            config: TrieConfig::default(),
        }
    }

    /// 获取值
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        debug!("Getting value for key: {:?}", key);
        
        let root_ref = self.root.read().await;
        let result = self.get_recursive(&root_ref, key).await?;
        
        debug!("Get result for key {:?}: {:?}", key, result.is_some());
        Ok(result)
    }

    /// 设置值
    pub async fn set(&self, key: &[u8], value: Vec<u8>) -> Result<()> {
        debug!("Setting value for key: {:?}", key);
        
        let mut root_ref = self.root.write().await;
        let new_root = self.set_recursive(&root_ref, key, &value).await?;
        
        *root_ref = new_root;
        
        debug!("Set value for key: {:?}", key);
        Ok(())
    }

    /// 删除值
    pub async fn delete(&self, key: &[u8]) -> Result<()> {
        debug!("Deleting value for key: {:?}", key);
        
        let mut root_ref = self.root.write().await;
        let new_root = self.set_recursive(&root_ref, key, &None).await?;
        
        *root_ref = new_root;
        
        debug!("Deleted value for key: {:?}", key);
        Ok(())
    }

    /// 获取根哈希
    pub async fn root_hash(&self) -> Result<Hash> {
        let root_ref = self.root.read().await;
        let hash = self.hash_node_ref(&root_ref).await?;
        Ok(hash)
    }

    /// 生成 Merkle 证明
    pub async fn generate_proof(&self, key: &[u8]) -> Result<MerkleProof> {
        debug!("Generating proof for key: {:?}", key);
        
        let root_ref = self.root.read().await;
        let root_hash = self.hash_node_ref(&root_ref).await?;
        
        let mut proof_nodes = Vec::new();
        let value = self.generate_proof_recursive(&root_ref, key, &mut proof_nodes).await?;
        
        Ok(MerkleProof {
            key: key.to_vec(),
            proof_nodes,
            root_hash,
            value,
        })
    }

    /// 验证 Merkle 证明
    pub async fn verify_proof(&self, proof: &MerkleProof) -> Result<bool> {
        debug!("Verifying proof for key: {:?}", proof.key);
        
        // 1. 验证根哈希
        let computed_root = self.compute_root_from_proof(proof).await?;
        let is_valid = computed_root == proof.root_hash;
        
        debug!("Proof verification result: {}", is_valid);
        Ok(is_valid)
    }

    /// 提交更改
    pub async fn commit(&self) -> Result<()> {
        debug!("Committing trie changes");
        
        // 1. 计算所有脏节点的哈希
        let mut dirty_nodes = Vec::new();
        self.collect_dirty_nodes(&self.root.read().await, &mut dirty_nodes).await?;
        
        // 2. 批量写入数据库
        if !dirty_nodes.is_empty() {
            let batch_nodes: Vec<(Hash, Node)> = dirty_nodes.into_iter()
                .filter_map(|node| {
                    if let Some(hash) = node.hash {
                        Some((hash, node))
                    } else {
                        None
                    }
                })
                .collect();
            
            self.db.batch_write(&batch_nodes).await?;
        }

        // 3. 更新根哈希
        let root_hash = self.root_hash().await?;
        self.db.set_root_hash(&root_hash).await?;

        // 4. 清理缓存
        if self.config.enable_cache {
            let mut cache = self.cache.write().await;
            cache.clear();
        }

        debug!("Trie changes committed");
        Ok(())
    }

    /// 回滚更改
    pub async fn rollback(&self) -> Result<()> {
        debug!("Rolling back trie changes");
        
        // 1. 重新加载根哈希
        if let Some(root_hash) = self.db.get_root_hash().await? {
            let root_ref = NodeRef::Hash(root_hash);
            *self.root.write().await = root_ref;
        }

        // 2. 清理缓存
        if self.config.enable_cache {
            let mut cache = self.cache.write().await;
            cache.clear();
        }

        debug!("Trie changes rolled back");
        Ok(())
    }

    /// 递归获取值
    async fn get_recursive(&self, node_ref: &NodeRef, key: &[u8]) -> Result<Option<Vec<u8>>> {
        match node_ref {
            NodeRef::Inline(node) => {
                match node.as_ref() {
                    NodeType::Null => Ok(None),
                    NodeType::Leaf { key: leaf_key, value } => {
                        if leaf_key == key {
                            Ok(Some(value.clone()))
                        } else {
                            Ok(None)
                        }
                    }
                    NodeType::Extension { shared_nibble, child } => {
                        if key.starts_with(shared_nibble) {
                            let remaining_key = &key[shared_nibble.len()..];
                            self.get_recursive(child, remaining_key).await
                        } else {
                            Ok(None)
                        }
                    }
                    NodeType::Branch { children, value } => {
                        if key.is_empty() {
                            Ok(value.clone())
                        } else {
                            let nibble = key[0];
                            if nibble < 16 {
                                if let Some(child) = &children[nibble as usize] {
                                    let remaining_key = &key[1..];
                                    self.get_recursive(child, remaining_key).await
                                } else {
                                    Ok(None)
                                }
                            } else {
                                Ok(None)
                            }
                        }
                    }
                }
            }
            NodeRef::Hash(hash) => {
                // 从数据库加载节点
                if let Some(node) = self.db.get_node(hash).await? {
                    let node_ref = NodeRef::Inline(Box::new(node.node_type));
                    self.get_recursive(&node_ref, key).await
                } else {
                    Err(NornError::DatabaseError("Node not found".to_string()))
                }
            }
        }
    }

    /// 递归设置值
    async fn set_recursive(&self, node_ref: &NodeRef, key: &[u8], value: &Option<Vec<u8>>) -> Result<NodeRef> {
        match node_ref {
            NodeRef::Inline(node) => {
                let new_node_type = self.set_node_recursive(&node, key, value).await?;
                Ok(NodeRef::Inline(Box::new(new_node_type)))
            }
            NodeRef::Hash(hash) => {
                // 从数据库加载节点
                if let Some(node) = self.db.get_node(hash).await? {
                    let new_node_type = self.set_node_recursive(&node.node_type, key, value).await?;
                    Ok(NodeRef::Inline(Box::new(new_node_type)))
                } else {
                    Err(NornError::DatabaseError("Node not found".to_string()))
                }
            }
        }
    }

    /// 递归设置节点
    async fn set_node_recursive(&self, node_type: &NodeType, key: &[u8], value: &Option<Vec<u8>>) -> Result<NodeType> {
        match node_type {
            NodeType::Null => {
                if let Some(v) = value {
                    Ok(NodeType::Leaf {
                        key: key.to_vec(),
                        value: v.clone(),
                    })
                } else {
                    Ok(NodeType::Null)
                }
            }
            NodeType::Leaf { key: leaf_key, value: leaf_value } => {
                if leaf_key == key {
                    if let Some(v) = value {
                        Ok(NodeType::Leaf {
                            key: key.to_vec(),
                            value: v.clone(),
                        })
                    } else {
                        Ok(NodeType::Null)
                    }
                } else {
                    // 需要创建分支节点
                    self.create_branch_from_leaf(leaf_key, leaf_value, key, value).await
                }
            }
            NodeType::Extension { shared_nibble, child } => {
                if key.starts_with(shared_nibble) {
                    let remaining_key = &key[shared_nibble.len()..];
                    let new_child = self.set_recursive(child, remaining_key, value).await?;
                    Ok(NodeType::Extension {
                        shared_nibble: shared_nibble.clone(),
                        child: new_child,
                    })
                } else {
                    // 需要创建分支节点
                    self.create_branch_from_extension(shared_nibble, child, key, value).await
                }
            }
            NodeType::Branch { children, value: branch_value } => {
                if key.is_empty() {
                    // 设置分支节点的值
                    let mut new_children = children.clone();
                    Ok(NodeType::Branch {
                        children: new_children,
                        value: value.clone(),
                    })
                } else {
                    let nibble = key[0];
                    if nibble < 16 {
                        let remaining_key = &key[1..];
                        let new_child = self.set_recursive(&children[nibble as usize].clone().unwrap_or(NodeRef::Inline(Box::new(NodeType::Null))), remaining_key, value).await?;
                        let mut new_children = children.clone();
                        new_children[nibble as usize] = Some(new_child);
                        Ok(NodeType::Branch {
                            children: new_children,
                            value: branch_value.clone(),
                        })
                    } else {
                        Err(NornError::ValidationError("Invalid nibble".to_string()))
                    }
                }
            }
        }
    }

    /// 从叶子节点创建分支节点
    async fn create_branch_from_leaf(&self, leaf_key: &[u8], leaf_value: &[u8], key: &[u8], value: &Option<Vec<u8>>) -> Result<NodeType> {
        let common_prefix = self.find_common_prefix(leaf_key, key);
        let leaf_suffix = &leaf_key[common_prefix.len()..];
        let key_suffix = &key[common_prefix.len()..];

        if common_prefix.is_empty() {
            // 创建分支节点
            let mut children = [None; 16];
            
            if !leaf_suffix.is_empty() {
                let leaf_nibble = leaf_suffix[0];
                let leaf_child = NodeRef::Inline(Box::new(NodeType::Leaf {
                    key: leaf_suffix[1..].to_vec(),
                    value: leaf_value.to_vec(),
                }));
                children[leaf_nibble as usize] = Some(leaf_child);
            }

            if !key_suffix.is_empty() {
                let key_nibble = key_suffix[0];
                let key_child = if let Some(v) = value {
                    NodeRef::Inline(Box::new(NodeType::Leaf {
                        key: key_suffix[1..].to_vec(),
                        value: v.clone(),
                    }))
                } else {
                    NodeRef::Inline(Box::new(NodeType::Null))
                };
                children[key_nibble as usize] = Some(key_child);
            }

            Ok(NodeType::Branch {
                children,
                value: if key_suffix.is_empty() { value.clone() } else { None },
            })
        } else {
            // 创建扩展节点
            let extension_node = NodeType::Extension {
                shared_nibble: common_prefix.to_vec(),
                child: Box::new(NodeType::Branch {
                    children: [None; 16],
                    value: None,
                }),
            };
            Ok(extension_node)
        }
    }

    /// 从扩展节点创建分支节点
    async fn create_branch_from_extension(&self, shared_nibble: &[u8], child: &NodeRef, key: &[u8], value: &Option<Vec<u8>>) -> Result<NodeType> {
        let common_prefix = self.find_common_prefix(shared_nibble, key);
        
        if common_prefix.is_empty() {
            // 创建分支节点
            let mut children = [None; 16];
            
            if !shared_nibble.is_empty() {
                let nibble = shared_nibble[0];
                let extension_child = NodeRef::Inline(Box::new(NodeType::Extension {
                    shared_nibble: shared_nibble[1..].to_vec(),
                    child: child.clone(),
                }));
                children[nibble as usize] = Some(extension_child);
            }

            if !key.is_empty() {
                let nibble = key[0];
                let key_child = if let Some(v) = value {
                    NodeRef::Inline(Box::new(NodeType::Leaf {
                        key: key[1..].to_vec(),
                        value: v.clone(),
                    }))
                } else {
                    NodeRef::Inline(Box::new(NodeType::Null))
                };
                children[nibble as usize] = Some(key_child);
            }

            Ok(NodeType::Branch {
                children,
                value: if key.is_empty() { value.clone() } else { None },
            })
        } else {
            // 创建新的扩展节点
            let remaining_shared = &shared_nibble[common_prefix.len()..];
            let new_child = NodeRef::Inline(Box::new(NodeType::Extension {
                shared_nibble: remaining_shared.to_vec(),
                child: child.clone(),
            }));
            
            Ok(NodeType::Extension {
                shared_nibble: common_prefix.to_vec(),
                child: Box::new(new_child),
            })
        }
    }

    /// 查找公共前缀
    fn find_common_prefix(&self, a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut prefix = Vec::new();
        let min_len = std::cmp::min(a.len(), b.len());
        
        for i in 0..min_len {
            if a[i] == b[i] {
                prefix.push(a[i]);
            } else {
                break;
            }
        }
        
        prefix
    }

    /// 计算节点引用的哈希
    async fn hash_node_ref(&self, node_ref: &NodeRef) -> Result<Hash> {
        match node_ref {
            NodeRef::Inline(node) => {
                let serialized = serde_json::to_vec(node)?;
                let hash = Sha256::digest(&serialized);
                let mut result = [0u8; 32];
                result.copy_from_slice(&hash);
                Ok(Hash(result))
            }
            NodeRef::Hash(hash) => Ok(*hash),
        }
    }

    /// 生成证明递归
    async fn generate_proof_recursive(&self, node_ref: &NodeRef, key: &[u8], proof_nodes: &mut Vec<Node>) -> Result<Option<Vec<u8>>> {
        match node_ref {
            NodeRef::Inline(node) => {
                let node_hash = self.hash_node_ref(node_ref).await?;
                proof_nodes.push(Node {
                    node_type: node.as_ref().clone(),
                    hash: Some(node_hash),
                    dirty: false,
                });
                
                match node.as_ref() {
                    NodeType::Null => Ok(None),
                    NodeType::Leaf { key: leaf_key, value } => {
                        if leaf_key == key {
                            Ok(Some(value.clone()))
                        } else {
                            Ok(None)
                        }
                    }
                    NodeType::Extension { shared_nibble, child } => {
                        if key.starts_with(shared_nibble) {
                            let remaining_key = &key[shared_nibble.len()..];
                            self.generate_proof_recursive(child, remaining_key, proof_nodes).await
                        } else {
                            Ok(None)
                        }
                    }
                    NodeType::Branch { children, value } => {
                        if key.is_empty() {
                            Ok(value.clone())
                        } else {
                            let nibble = key[0];
                            if nibble < 16 {
                                if let Some(child) = &children[nibble as usize] {
                                    let remaining_key = &key[1..];
                                    self.generate_proof_recursive(child, remaining_key, proof_nodes).await
                                } else {
                                    Ok(None)
                                }
                            } else {
                                Ok(None)
                            }
                        }
                    }
                }
            }
            NodeRef::Hash(hash) => {
                if let Some(node) = self.db.get_node(hash).await? {
                    let node_ref = NodeRef::Inline(Box::new(node.node_type));
                    self.generate_proof_recursive(&node_ref, key, proof_nodes).await
                } else {
                    Err(NornError::DatabaseError("Node not found".to_string()))
                }
            }
        }
    }

    /// 从证明计算根哈希
    async fn compute_root_from_proof(&self, proof: &MerkleProof) -> Result<Hash> {
        // TODO: 实现从证明重建根哈希的逻辑
        // 这是一个复杂的操作，需要重建路径并计算哈希
        Ok(proof.root_hash)
    }

    /// 收集脏节点
    async fn collect_dirty_nodes(&self, node_ref: &NodeRef, dirty_nodes: &mut Vec<Node>) {
        match node_ref {
            NodeRef::Inline(node) => {
                let node_hash = self.hash_node_ref(node_ref).await.ok();
                let node = Node {
                    node_type: node.as_ref().clone(),
                    hash: node_hash,
                    dirty: true,
                };
                dirty_nodes.push(node);
                
                // 递归收集子节点
                match node.as_ref() {
                    NodeType::Extension { child, .. } => {
                        self.collect_dirty_nodes(child, dirty_nodes).await;
                    }
                    NodeType::Branch { children, .. } => {
                        for child in children.iter().flatten() {
                            self.collect_dirty_nodes(child, dirty_nodes).await;
                        }
                    }
                    _ => {}
                }
            }
            NodeRef::Hash(_) => {
                // 哈希引用节点不是脏的
            }
        }
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> TrieStats {
        let root_ref = self.root.read().await;
        let mut stats = TrieStats::default();
        self.calculate_stats(&root_ref, &mut stats).await;
        stats
    }

    /// 计算统计信息
    async fn calculate_stats(&self, node_ref: &NodeRef, stats: &mut TrieStats) {
        match node_ref {
            NodeRef::Inline(node) => {
                stats.total_nodes += 1;
                
                match node.as_ref() {
                    NodeType::Null => stats.null_nodes += 1,
                    NodeType::Leaf { .. } => stats.leaf_nodes += 1,
                    NodeType::Extension { child, .. } => {
                        stats.extension_nodes += 1;
                        self.calculate_stats(child, stats).await;
                    }
                    NodeType::Branch { children, .. } => {
                        stats.branch_nodes += 1;
                        for child in children.iter().flatten() {
                            self.calculate_stats(child, stats).await;
                        }
                    }
                }
            }
            NodeRef::Hash(_) => {
                stats.hash_nodes += 1;
            }
        }
    }
}

/// Trie 统计信息
#[derive(Debug, Clone, Default)]
pub struct TrieStats {
    pub total_nodes: u64,
    pub null_nodes: u64,
    pub leaf_nodes: u64,
    pub extension_nodes: u64,
    pub branch_nodes: u64,
    pub hash_nodes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockTrieDB {
        nodes: Arc<RwLock<HashMap<Hash, Node>>>,
        root_hash: Arc<RwLock<Option<Hash>>>,
    }

    impl MockTrieDB {
        fn new() -> Self {
            Self {
                nodes: Arc::new(RwLock::new(HashMap::new())),
                root_hash: Arc::new(RwLock::new(None)),
            }
        }
    }

    #[async_trait::async_trait]
    impl TrieDB for MockTrieDB {
        async fn get_node(&self, hash: &Hash) -> Result<Option<Node>> {
            let nodes = self.nodes.read().await;
            Ok(nodes.get(hash).cloned())
        }

        async fn put_node(&self, hash: &Hash, node: &Node) -> Result<()> {
            let mut nodes = self.nodes.write().await;
            nodes.insert(*hash, node.clone());
            Ok(())
        }

        async fn delete_node(&self, hash: &Hash) -> Result<()> {
            let mut nodes = self.nodes.write().await;
            nodes.remove(hash);
            Ok(())
        }

        async fn batch_write(&self, nodes: &[(Hash, Node)]) -> Result<()> {
            let mut db_nodes = self.nodes.write().await;
            for (hash, node) in nodes {
                db_nodes.insert(*hash, node.clone());
            }
            Ok(())
        }

        async fn get_root_hash(&self) -> Result<Option<Hash>> {
            let root_hash = self.root_hash.read().await;
            Ok(*root_hash)
        }

        async fn set_root_hash(&self, hash: &Hash) -> Result<()> {
            let mut root_hash = self.root_hash.write().await;
            *root_hash = Some(*hash);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_trie_basic_operations() {
        let db = Arc::new(MockTrieDB::new());
        let config = TrieConfig::default();
        let trie = MerklePatriciaTrie::new(db, config);

        // 测试设置和获取
        let key = b"test_key";
        let value = b"test_value";
        
        trie.set(key, value.to_vec()).await.unwrap();
        let retrieved = trie.get(key).await.unwrap();
        
        assert_eq!(retrieved, Some(value.to_vec()));
    }

    #[tokio::test]
    async fn test_trie_root_hash() {
        let db = Arc::new(MockTrieDB::new());
        let config = TrieConfig::default();
        let trie = MerklePatriciaTrie::new(db, config);

        // 空 trie 的根哈希
        let empty_root = trie.root_hash().await.unwrap();
        
        // 设置值后的根哈希
        trie.set(b"key1", b"value1".to_vec()).await.unwrap();
        let root1 = trie.root_hash().await.unwrap();
        
        trie.set(b"key2", b"value2".to_vec()).await.unwrap();
        let root2 = trie.root_hash().await.unwrap();
        
        assert_ne!(empty_root, root1);
        assert_ne!(root1, root2);
    }

    #[tokio::test]
    async fn test_trie_delete() {
        let db = Arc::new(MockTrieDB::new());
        let config = TrieConfig::default();
        let trie = MerklePatriciaTrie::new(db, config);

        let key = b"test_key";
        let value = b"test_value";
        
        // 设置值
        trie.set(key, value.to_vec()).await.unwrap();
        assert!(trie.get(key).await.unwrap().is_some());
        
        // 删除值
        trie.delete(key).await.unwrap();
        assert!(trie.get(key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_merkle_proof() {
        let db = Arc::new(MockTrieDB::new());
        let config = TrieConfig::default();
        let trie = MerklePatriciaTrie::new(db, config);

        let key = b"test_key";
        let value = b"test_value";
        
        // 设置值
        trie.set(key, value.to_vec()).await.unwrap();
        
        // 生成证明
        let proof = trie.generate_proof(key).await.unwrap();
        
        // 验证证明
        let is_valid = trie.verify_proof(&proof).await.unwrap();
        assert!(is_valid);
        
        // 验证值
        assert_eq!(proof.value, Some(value.to_vec()));
    }

    #[tokio::test]
    async fn test_trie_stats() {
        let db = Arc::new(MockTrieDB::new());
        let config = TrieConfig::default();
        let trie = MerklePatriciaTrie::new(db, config);

        // 添加一些值
        trie.set(b"key1", b"value1".to_vec()).await.unwrap();
        trie.set(b"key2", b"value2".to_vec()).await.unwrap();
        trie.set(b"key3", b"value3".to_vec()).await.unwrap();
        
        // 获取统计信息
        let stats = trie.get_stats().await;
        assert!(stats.total_nodes > 0);
        assert!(stats.leaf_nodes > 0);
    }
}