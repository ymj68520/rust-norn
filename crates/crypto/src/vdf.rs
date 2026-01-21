use norn_common::types::{Hash, GeneralParams};
use num_bigint::BigInt;
use num_traits::{Zero, One};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::sync::Mutex;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use sha2::{Sha256, Digest};
use std::time::{Duration, Instant};
use async_trait::async_trait;

/// VDF 模数 - 使用 secp256k1 素数以确保安全性
const VDF_MODULUS_HEX: &str = "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F";

/// 最大迭代次数限制，防止 DoS 攻击
const MAX_VDF_ITERATIONS: u64 = 10_000_000;

/// VDF 计算器特征
#[async_trait::async_trait]
pub trait VDFCalculator: Send + Sync + std::fmt::Debug {
    /// 计算 VDF
    async fn compute_vdf(&self, input: &Hash, params: &GeneralParams) -> Result<VDFOutput, Box<dyn std::error::Error + Send + Sync>>;
    
    /// 验证 VDF
    async fn verify_vdf(&self, input: &Hash, output: &VDFOutput, params: &GeneralParams) -> bool;
    
    /// 获取计算器名称
    fn name(&self) -> &'static str;
}

/// VDF 输出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VDFOutput {
    pub proof: Vec<u8>,
    pub result: Hash,
    pub iterations: u64,
    pub computation_time: Duration,
}

/// VDF 计算状态
#[derive(Debug, Clone)]
pub struct VDFState {
    pub current_iteration: u64,
    pub current_value: BigInt,
    pub is_completed: bool,
    pub start_time: Instant,
}

/// 简化的 VDF 实现（基于平方迭代）
#[derive(Clone, Debug)]
pub struct SimpleVDF {
    cache: Arc<RwLock<HashMap<Hash, VDFOutput>>>,
}

impl SimpleVDF {
    /// 创建新的 VDF 计算器
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 从缓存获取 VDF 输出
    pub async fn get_cached_output(&self, input: &Hash) -> Option<VDFOutput> {
        let cache = self.cache.read().await;
        cache.get(input).cloned()
    }

    /// 缓存 VDF 输出
    pub async fn cache_output(&self, input: &Hash, output: VDFOutput) {
        let mut cache = self.cache.write().await;
        cache.insert(*input, output);
    }

    /// 执行 VDF 计算
    async fn compute_vdf_internal(&self, input: &Hash, iterations: u64) -> Result<VDFOutput, Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting VDF computation for {} iterations", iterations);
        let start_time = Instant::now();

        // 检查迭代次数上限
        if iterations > MAX_VDF_ITERATIONS {
            return Err(format!("Iterations {} exceeds maximum {}", iterations, MAX_VDF_ITERATIONS).into());
        }

        // 检查迭代次数上限
        if iterations > MAX_VDF_ITERATIONS {
            return Err(format!("Iterations {} exceeds maximum {}", iterations, MAX_VDF_ITERATIONS).into());
        }

        // 1. 将输入哈希转换为 BigInt
        let input_bigint = self.hash_to_bigint(input)?;
        debug!("Input as BigInt: {}", input_bigint);

        // 2. 获取 VDF 模数
        let modulus = BigInt::parse_bytes(VDF_MODULUS_HEX.as_bytes(), 16)
            .ok_or("Failed to parse VDF modulus")?;

        // 3. 执行 VDF 计算：y = x^(2^t) mod p
        let mut current_value = input_bigint.clone() % &modulus;
        // 2. 获取 VDF 模数
        let modulus = BigInt::parse_bytes(VDF_MODULUS_HEX.as_bytes(), 16)
            .ok_or("Failed to parse VDF modulus")?;

        // 3. 执行 VDF 计算：y = x^(2^t) mod p
        let mut current_value = input_bigint.clone() % &modulus;
        let mut current_iteration = 0u64;

        // 分批计算以避免长时间阻塞
        const BATCH_SIZE: u64 = 10000;
        let mut proof_steps = Vec::new();

        while current_iteration < iterations {
            let batch_end = std::cmp::min(current_iteration + BATCH_SIZE, iterations);

            for _ in current_iteration..batch_end {
                // VDF：模平方运算 (y = x^2 mod p)
                current_value = (&current_value * &current_value) % &modulus;
                current_iteration += 1;
            }

            // 每批次记录一次中间值作为证明
            proof_steps.push(current_value.clone());

            // 让出控制权以避免阻塞
            tokio::task::yield_now().await;
        }

        // 3. 生成最终结果
        let result_hash = self.bigint_to_hash(&current_value)?;
        
        // 4. 生成证明
        let proof = self.generate_proof(&proof_steps, &current_value)?;

        let computation_time = start_time.elapsed();
        info!("VDF computation completed in {:?}", computation_time);

        Ok(VDFOutput {
            proof,
            result: result_hash,
            iterations,
            computation_time,
        })
    }

    /// 将哈希转换为 BigInt
    fn hash_to_bigint(&self, hash: &Hash) -> Result<BigInt, Box<dyn std::error::Error + Send + Sync>> {
        let hex_string = hex::encode(hash.0);
        BigInt::parse_bytes(hex_string.as_bytes(), 16)
            .ok_or_else(|| "Failed to parse hash as BigInt".into())
    }

    /// 将 BigInt 转换为哈希
    fn bigint_to_hash(&self, value: &BigInt) -> Result<Hash, Box<dyn std::error::Error + Send + Sync>> {
        let mut hex_string = value.to_str_radix(16);
        if hex_string.len() % 2 != 0 {
            hex_string.insert(0, '0');
        }
        let bytes = hex::decode(&hex_string)
            .map_err(|e| format!("Failed to convert BigInt to hex: {}", e))?;
        
        if bytes.len() < 32 {
            let mut padded = vec![0u8; 32];
            padded[(32 - bytes.len())..].copy_from_slice(&bytes);
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&padded);
            Ok(Hash(hash))
        } else {
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&bytes[..32]);
            Ok(Hash(hash))
        }
    }

    /// 生成 VDF 证明
    fn generate_proof(&self, steps: &[BigInt], final_value: &BigInt) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        // 简化的证明：包含关键中间步骤
        let mut proof = Vec::new();
        
        // 添加步骤数量
        proof.extend_from_slice(&(steps.len() as u64).to_le_bytes());
        
        // 添加每一步的哈希
        for step in steps.iter().take(10) { // 只保存前 10 步
            let step_hash = Sha256::digest(step.to_string().as_bytes());
            proof.extend_from_slice(&step_hash);
        }
        
        // 添加最终值的哈希
        let final_hash = Sha256::digest(final_value.to_string().as_bytes());
        proof.extend_from_slice(&final_hash);
        
        Ok(proof)
    }
}

#[async_trait]
impl VDFCalculator for SimpleVDF {
    async fn compute_vdf(&self, input: &Hash, params: &GeneralParams) -> Result<VDFOutput, Box<dyn std::error::Error + Send + Sync>> {
        // 1. 检查缓存
        if let Some(cached) = self.get_cached_output(input).await {
            debug!("Using cached VDF output for input: {}", input);
            return Ok(cached);
        }

        // 2. 解析参数
        let iterations = self.extract_iterations(params)?;
        debug!("VDF iterations: {}", iterations);

        // 3. 执行计算
        let output = self.compute_vdf_internal(input, iterations).await?;

        // 4. 缓存结果
        self.cache_output(input, output.clone()).await;

        Ok(output)
    }

    async fn verify_vdf(&self, input: &Hash, output: &VDFOutput, params: &GeneralParams) -> bool {
        debug!("Verifying VDF output for input: {}", input);

        // 1. 验证迭代次数
        let expected_iterations = match self.extract_iterations(params) {
            Ok(iterations) => iterations,
            Err(e) => {
                error!("Failed to extract iterations from params: {}", e);
                return false;
            }
        };

        if output.iterations != expected_iterations {
            warn!("Iteration count mismatch: expected {}, got {}", expected_iterations, output.iterations);
            return false;
        }

        // 2. 验证证明
        if !self.verify_proof(&output.proof, input, &output.result) {
            warn!("VDF proof verification failed");
            return false;
        }

        // 3. 重新计算并验证结果
        match self.compute_vdf_internal(input, expected_iterations).await {
            Ok(expected_output) => {
                let is_valid = expected_output.result == output.result;
                if is_valid {
                    debug!("VDF verification successful");
                } else {
                    warn!("VDF result mismatch");
                }
                is_valid
            }
            Err(e) => {
                error!("Failed to recompute VDF for verification: {}", e);
                false
            }
        }
    }

    fn name(&self) -> &'static str {
        "SimpleVDF"
    }
}

impl SimpleVDF {
    /// 从参数中提取迭代次数（解析为 little-endian u64）
    fn extract_iterations(&self, params: &GeneralParams) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        if params.t.len() >= 8 {
            // 正确解析为 little-endian u64
            let bytes: [u8; 8] = params.t[..8].try_into()
                .map_err(|_| "Invalid t parameter length")?;
            Ok(u64::from_le_bytes(bytes))
        } else if !params.t.is_empty() {
            // 兼容较短的字节数组
            let mut bytes = [0u8; 8];
            bytes[..params.t.len()].copy_from_slice(&params.t);
            Ok(u64::from_le_bytes(bytes))
        } else {
            Err("Invalid time parameter in VDF params".into())
        }
    }

    /// 验证 VDF 证明
    fn verify_proof(&self, proof: &[u8], _input: &Hash, result: &Hash) -> bool {
        if proof.len() < 8 {
            return false;
        }

        // 1. 提取步骤数量
        let step_count = u64::from_le_bytes([
            proof[0], proof[1], proof[2], proof[3],
            proof[4], proof[5], proof[6], proof[7],
        ]);

        // 验证证明大小
        let expected_proof_size = (8 + step_count.min(10) * 32 + 32) as usize; // 步数 + 最多10步哈希 + 最终哈希
        if proof.len() != expected_proof_size {
            warn!("Proof size mismatch: expected {}, got {}", expected_proof_size, proof.len());
            return false;
        }

        // 验证最终哈希
        let final_hash_offset = (8 + step_count.min(10) * 32) as usize;
        if final_hash_offset + 32 > proof.len() {
            return false;
        }

        let stored_final_hash = &proof[final_hash_offset..final_hash_offset + 32];

        // 将结果哈希转换为 BigInt，与 generate_proof 中的方式一致
        let result_bigint = match self.hash_to_bigint(result) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let computed_final_hash = Sha256::digest(result_bigint.to_string().as_bytes());

        stored_final_hash == computed_final_hash.as_slice()
    }
}

/// VDF 计算管理器
pub struct VDFManager {
    calculator: Arc<dyn VDFCalculator>,
    active_computations: Arc<RwLock<HashMap<Hash, VDFState>>>,
}

impl VDFManager {
    /// 创建新的 VDF 管理器
    pub fn new(calculator: Arc<dyn VDFCalculator>) -> Self {
        Self {
            calculator,
            active_computations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 启动异步 VDF 计算
    pub async fn start_computation(
        &self,
        input: Hash,
        params: GeneralParams,
    ) -> Result<Hash, Box<dyn std::error::Error + Send + Sync>> {
        let iterations = self.extract_iterations(&params)?;
        
        // 1. 检查是否已有活跃计算
        {
            let active = self.active_computations.read().await;
            if let Some(state) = active.get(&input) {
                if state.is_completed {
                    // 计算已完成，返回结果
                    let (sign, bytes_vec) = state.current_value.to_bytes_be();
                    return Ok(Hash::from_slice(&bytes_vec));
                }
            }
        }

        // 2. 创建新的计算状态
        let input_bigint = self.hash_to_bigint(&input)?;
        let state = VDFState {
            current_iteration: 0,
            current_value: input_bigint,
            is_completed: false,
            start_time: Instant::now(),
        };

        {
            let mut active = self.active_computations.write().await;
            active.insert(input, state);
        }

        // 3. 执行计算
        let output = self.calculator.compute_vdf(&input, &params).await?;

        // 4. 更新状态
        {
            let mut active = self.active_computations.write().await;
            if let Some(state) = active.get_mut(&input) {
                state.is_completed = true;
                state.current_value = self.hash_to_bigint(&output.result)?;
            }
        }

        Ok(output.result)
    }

    /// 获取计算状态
    pub async fn get_computation_state(&self, input: &Hash) -> Option<VDFState> {
        let active = self.active_computations.read().await;
        active.get(input).cloned()
    }

    /// 取消计算
    pub async fn cancel_computation(&self, input: &Hash) -> bool {
        let mut active = self.active_computations.write().await;
        active.remove(input).is_some()
    }

    /// 清理已完成的计算
    pub async fn cleanup_completed(&self, max_age: Duration) {
        let mut active = self.active_computations.write().await;
        let now = Instant::now();
        
        active.retain(|_, state| {
            now.duration_since(state.start_time) < max_age && !state.is_completed
        });
    }

    /// 从参数提取迭代次数（解析为 little-endian u64）
    fn extract_iterations(&self, params: &GeneralParams) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        if params.t.len() >= 8 {
            // 正确解析为 little-endian u64
            let bytes: [u8; 8] = params.t[..8].try_into()
                .map_err(|_| "Invalid t parameter length")?;
            Ok(u64::from_le_bytes(bytes))
        } else if !params.t.is_empty() {
            // 兼容较短的字节数组
            let mut bytes = [0u8; 8];
            bytes[..params.t.len()].copy_from_slice(&params.t);
            Ok(u64::from_le_bytes(bytes))
        } else {
            Err("Invalid time parameter".into())
        }
    }

    /// 哈希转 BigInt
    fn hash_to_bigint(&self, hash: &Hash) -> Result<BigInt, Box<dyn std::error::Error + Send + Sync>> {
        let hex_string = hex::encode(hash.0);
        BigInt::parse_bytes(hex_string.as_bytes(), 16)
            .ok_or_else(|| "Failed to parse hash as BigInt".into())
    }
}

/// 全局 VDF 计算器实例
static VDF_CALCULATOR: std::sync::OnceLock<Arc<dyn VDFCalculator>> = std::sync::OnceLock::new();

/// 获取全局 VDF 计算器
pub fn get_calculator() -> Option<Arc<dyn VDFCalculator>> {
    VDF_CALCULATOR.get().cloned()
}

/// 初始化全局 VDF 计算器
pub fn init_calculator() -> Arc<dyn VDFCalculator> {
    let calculator = Arc::new(SimpleVDF::new());
    VDF_CALCULATOR.set(calculator.clone()).unwrap();
    calculator
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::{GenesisParams, PublicKey};

    fn create_test_params() -> GeneralParams {
        GeneralParams {
            result: vec![],
            random_number: PublicKey::default(),
            s: vec![],
            t: vec![0xE8, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // 1000 as little-endian bytes
            proof: vec![],
        }
    }

    #[tokio::test]
    async fn test_vdf_computation() {
        let calculator = SimpleVDF::new();
        let input = Hash([1u8; 32]);

        let params = create_test_params();

        let result = calculator.compute_vdf(&input, &params).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.iterations > 0);
        assert!(!output.proof.is_empty());
    }

    #[tokio::test]
    async fn test_vdf_verification() {
        let calculator = SimpleVDF::new();
        let input = Hash([1u8; 32]);

        let params = create_test_params();

        // 计算输出
        let output = calculator.compute_vdf(&input, &params).await.unwrap();

        // 验证输出
        let is_valid = calculator.verify_vdf(&input, &output, &params).await;
        assert!(is_valid);
    }

    #[tokio::test]
    async fn test_vdf_caching() {
        let calculator = SimpleVDF::new();
        let input = Hash([1u8; 32]);

        let params = create_test_params();

        // 第一次计算
        let result1 = calculator.compute_vdf(&input, &params).await.unwrap();

        // 第二次计算应该使用缓存
        let result2 = calculator.compute_vdf(&input, &params).await.unwrap();

        assert_eq!(result1.result, result2.result);
        assert_eq!(result1.iterations, result2.iterations);
    }

    #[tokio::test]
    async fn test_vdf_manager() {
        let calculator = Arc::new(SimpleVDF::new());
        let manager = VDFManager::new(calculator);

        let input = Hash([1u8; 32]);
        let params = create_test_params();

        // 启动计算
        let result = manager.start_computation(input, params).await;
        assert!(result.is_ok());

        // 检查状态
        let state = manager.get_computation_state(&input).await;
        assert!(state.is_some());
        assert!(state.unwrap().is_completed);
    }
}