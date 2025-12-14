// VRF（可验证随机函数）模块
// 
// 实现基于 ECDSA 的可验证随机函数，用于 PoVF 共识中的随机数生成

use anyhow::{Result, anyhow};
use curve25519_dalek::{
    ristretto::RistrettoPoint,
    scalar::Scalar,
};
use rand::rngs::OsRng;
use rand_core::RngCore;
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use tracing::{info, warn};

// 重新定义类型以避免依赖问题
pub type Address = [u8; 20];
pub type StakeAmount = u64;

// 辅助函数：将地址转换为十六进制字符串
fn address_to_hex(address: &Address) -> String {
    hex::encode(address)
}

/// VRF 密钥对
#[derive(Debug, Clone)]
pub struct VRFKeyPair {
    /// 私钥
    pub private_key: Scalar,
    /// 公钥
    pub public_key: RistrettoPoint,
}

impl VRFKeyPair {
    /// 生成新的 VRF 密钥对
    pub fn generate() -> Self {
        let mut csprng = OsRng;
        let mut private_key_bytes = [0u8; 32];
        csprng.fill_bytes(&mut private_key_bytes);
        let private_key = Scalar::from_bytes_mod_order(private_key_bytes);
        let public_key = RistrettoPoint::mul_base(&private_key);
        
        Self {
            private_key,
            public_key,
        }
    }

    /// 从种子生成密钥对
    pub fn from_seed(seed: &[u8]) -> Self {
        let mut hasher = Sha512::new();
        hasher.update(seed);
        let hash = hasher.finalize();
        
        let mut private_key_bytes = [0u8; 32];
        private_key_bytes.copy_from_slice(&hash[..32]);
        let private_key = Scalar::from_bytes_mod_order(private_key_bytes);
        let public_key = RistrettoPoint::mul_base(&private_key);
        
        Self {
            private_key,
            public_key,
        }
    }

    /// 获取公钥的字节表示
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.public_key.compress().to_bytes()
    }

    /// 获取私钥的字节表示
    pub fn private_key_bytes(&self) -> [u8; 32] {
        self.private_key.to_bytes()
    }
}

/// VRF 输出
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VRFOutput {
    /// VRF 输出值（32字节）
    pub output: [u8; 32],
    /// 证明
    pub proof: VRFProof,
}

/// VRF 证明
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VRFProof {
    /// 证明点
    pub gamma: RistrettoPoint,
    /// 挑战标量
    pub challenge: Scalar,
    /// 响应标量
    pub response: Scalar,
}

impl VRFProof {
    /// 序列化证明
    pub fn to_bytes(&self) -> [u8; 96] {
        let mut bytes = [0u8; 96];
        bytes[0..32].copy_from_slice(&self.gamma.compress().to_bytes());
        bytes[32..64].copy_from_slice(&self.challenge.to_bytes());
        bytes[64..96].copy_from_slice(&self.response.to_bytes());
        bytes
    }

    /// 从字节反序列化证明
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 96 {
            return Err(anyhow!("Invalid proof length"));
        }

        let gamma_bytes: [u8; 32] = bytes[0..32].try_into()
            .map_err(|_| anyhow!("Invalid gamma bytes"))?;
        let challenge_bytes: [u8; 32] = bytes[32..64].try_into()
            .map_err(|_| anyhow!("Invalid challenge bytes"))?;
        let response_bytes: [u8; 32] = bytes[64..96].try_into()
            .map_err(|_| anyhow!("Invalid response bytes"))?;

        let gamma_compressed = curve25519_dalek::ristretto::CompressedRistretto(gamma_bytes);
        let gamma = gamma_compressed.decompress()
            .ok_or_else(|| anyhow!("Invalid gamma point"))?;

        let challenge = Scalar::from_bytes_mod_order(challenge_bytes);
        let response = Scalar::from_bytes_mod_order(response_bytes);

        Ok(Self {
            gamma,
            challenge,
            response,
        })
    }
}

/// VRF 计算器
pub struct VRFCalculator;

impl VRFCalculator {
    /// 计算 VRF 输出和证明
    pub fn calculate(key_pair: &VRFKeyPair, message: &[u8]) -> Result<VRFOutput> {
        // 1. 计算 H = H(message)
        let h = Self::hash_to_curve(message)?;

        // 2. 计算 gamma = H^sk
        let gamma = RistrettoPoint::mul_base(&key_pair.private_key) + h;

        // 3. 生成随机数 k
        let mut csprng = OsRng;
        let mut k_bytes = [0u8; 32];
        csprng.fill_bytes(&mut k_bytes);
        let k = Scalar::from_bytes_mod_order(k_bytes);

        // 4. 计算 commitment = gamma^k
        let commitment = gamma * k;

        // 5. 计算挑战 c = H(pk || H || gamma || commitment)
        let challenge = Self::compute_challenge(
            &key_pair.public_key,
            &h,
            &gamma,
            &commitment,
        );

        // 6. 计算响应 s = k + c * sk
        let response = k + challenge * key_pair.private_key;

        // 7. 计算 VRF 输出
        let output = Self::derive_output(&gamma, &h);

        let proof = VRFProof {
            gamma,
            challenge,
            response,
        };

        Ok(VRFOutput { output, proof })
    }

    /// 验证 VRF 输出和证明
    pub fn verify(
        public_key: &RistrettoPoint,
        message: &[u8],
        output: &VRFOutput,
    ) -> Result<bool> {
        // 1. 计算 H = H(message)
        let h = Self::hash_to_curve(message)?;

        // 2. 重新计算 commitment
        let commitment_recomputed = output.proof.gamma * output.proof.response 
            - (RistrettoPoint::mul_base(&output.proof.response) + h * output.proof.challenge);

        // 3. 重新计算挑战
        let challenge_recomputed = Self::compute_challenge(
            public_key,
            &h,
            &output.proof.gamma,
            &commitment_recomputed,
        );

        // 4. 验证挑战
        if challenge_recomputed != output.proof.challenge {
            return Ok(false);
        }

        // 5. 验证输出
        let expected_output = Self::derive_output(&output.proof.gamma, &h);
        
        Ok(expected_output == output.output)
    }

    /// 将消息哈希到椭圆曲线上
    fn hash_to_curve(message: &[u8]) -> Result<RistrettoPoint> {
        let mut hasher = Sha512::new();
        hasher.update(b"VRF_HASH_TO_CURVE");
        hasher.update(message);
        let hash = hasher.finalize();

        // 使用哈希值生成标量，然后映射到曲线
        let mut scalar_bytes = [0u8; 32];
        scalar_bytes.copy_from_slice(&hash[..32]);
        let scalar = Scalar::from_bytes_mod_order(scalar_bytes);
        Ok(RistrettoPoint::mul_base(&scalar))
    }

    /// 计算挑战
    fn compute_challenge(
        pk: &RistrettoPoint,
        h: &RistrettoPoint,
        gamma: &RistrettoPoint,
        commitment: &RistrettoPoint,
    ) -> Scalar {
        let mut hasher = Sha512::new();
        hasher.update(b"VRF_CHALLENGE");
        hasher.update(pk.compress().to_bytes());
        hasher.update(h.compress().to_bytes());
        hasher.update(gamma.compress().to_bytes());
        hasher.update(commitment.compress().to_bytes());
        let hash = hasher.finalize();

        let mut challenge_bytes = [0u8; 32];
        challenge_bytes.copy_from_slice(&hash[..32]);
        Scalar::from_bytes_mod_order(challenge_bytes)
    }

    /// 从 gamma 和 H 推导输出
    fn derive_output(gamma: &RistrettoPoint, h: &RistrettoPoint) -> [u8; 32] {
        let mut hasher = Sha512::new();
        hasher.update(b"VRF_OUTPUT");
        hasher.update(gamma.compress().as_bytes());
        hasher.update(h.compress().as_bytes());
        let hash = hasher.finalize();

        let mut output = [0u8; 32];
        output.copy_from_slice(&hash[..32]);
        output
    }
}

/// VRF 选择器 - 用于基于权益的随机选择
pub struct VRFSelector {
    /// 验证者权益映射
    validators: HashMap<Address, StakeAmount>,
    /// VRF 密钥映射
    key_pairs: HashMap<Address, VRFKeyPair>,
}

impl VRFSelector {
    /// 创建新的 VRF 选择器
    pub fn new() -> Self {
        Self {
            validators: HashMap::new(),
            key_pairs: HashMap::new(),
        }
    }

    /// 添加验证者
    pub fn add_validator(&mut self, address: Address, stake: StakeAmount, key_pair: VRFKeyPair) {
        self.validators.insert(address, stake);
        self.key_pairs.insert(address, key_pair);
        info!("添加验证者: {} (权益: {})", address_to_hex(&address), stake);
    }

    /// 移除验证者
    pub fn remove_validator(&mut self, address: &Address) {
        self.validators.remove(address);
        self.key_pairs.remove(address);
        info!("移除验证者: {}", address_to_hex(address));
    }

    /// 选择提议者
    pub fn select_proposer(&self, message: &[u8], round: u64) -> Result<(Address, VRFOutput)> {
        if self.validators.is_empty() {
            return Err(anyhow!("没有可用的验证者"));
        }

        // 计算总权益
        let total_stake: StakeAmount = self.validators.values().sum();
        if total_stake == 0 {
            return Err(anyhow!("总权益为零"));
        }

        // 为每个验证者生成 VRF 输出
        let mut candidates = Vec::new();
        for (address, key_pair) in &self.key_pairs {
            let vrf_message = Self::create_selection_message(message, round, address);
            match VRFCalculator::calculate(key_pair, &vrf_message) {
                Ok(output) => {
                    let stake = self.validators.get(address).unwrap();
                    candidates.push((*address, output, *stake));
                }
                Err(e) => {
                    warn!("验证者 {} VRF 计算失败: {:?}", address_to_hex(&address), e);
                }
            }
        }

        if candidates.is_empty() {
            return Err(anyhow!("没有有效的候选者"));
        }

        // 选择具有最低 VRF 输出的验证者（考虑权益权重）
        let mut best_candidate = None;
        let mut best_score = None;

        for (address, output, stake) in candidates {
            let score = Self::calculate_vrf_score(&output.output, stake, total_stake);
            
            match best_score {
                None => {
                    best_score = Some(score);
                    best_candidate = Some((address, output));
                }
                Some(best) => {
                    if score < best {
                        best_score = Some(score);
                        best_candidate = Some((address, output));
                    }
                }
            }
        }

        best_candidate.ok_or_else(|| anyhow!("选择失败"))
    }

    /// 验证提议者选择
    pub fn verify_selection(
        &self,
        proposer: Address,
        message: &[u8],
        round: u64,
        output: &VRFOutput,
    ) -> Result<bool> {
        // 检查验证者是否存在
        let public_key = match self.key_pairs.get(&proposer) {
            Some(key_pair) => key_pair.public_key,
            None => return Ok(false),
        };

        let stake = match self.validators.get(&proposer) {
            Some(stake) => *stake,
            None => return Ok(false),
        };

        // 验证 VRF
        let vrf_message = Self::create_selection_message(message, round, &proposer);
        let vrf_valid = VRFCalculator::verify(&public_key, &vrf_message, output)?;

        if !vrf_valid {
            return Ok(false);
        }

        // 验证选择是否有效（这里可以实现更复杂的验证逻辑）
        let total_stake: StakeAmount = self.validators.values().sum();
        let score = Self::calculate_vrf_score(&output.output, stake, total_stake);
        
        // 这里可以实现阈值检查等
        Ok(true)
    }

    /// 创建选择消息
    fn create_selection_message(message: &[u8], round: u64, address: &Address) -> Vec<u8> {
        let mut vrf_message = Vec::new();
        vrf_message.extend_from_slice(b"VRF_SELECTOR");
        vrf_message.extend_from_slice(message);
        vrf_message.extend_from_slice(&round.to_be_bytes());
        vrf_message.extend_from_slice(address);
        vrf_message
    }

    /// 计算 VRF 分数（考虑权益权重）
    fn calculate_vrf_score(vrf_output: &[u8], stake: StakeAmount, total_stake: StakeAmount) -> f64 {
        if total_stake == 0 {
            return 1.0;
        }

        // 将 VRF 输出的前 8 字节作为 u64
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&vrf_output[..8]);
        let vrf_value = u64::from_be_bytes(bytes);

        // 归一化到 [0, 1)
        let normalized_vrf = vrf_value as f64 / u64::MAX as f64;

        // 应用权益权重：权益越高，有效分数越低
        let stake_ratio = stake as f64 / total_stake as f64;
        
        // 使用对数函数来平衡权益影响
        let stake_factor = 1.0 - (stake_ratio.ln() / (total_stake as f64).ln()).max(0.0);

        normalized_vrf * stake_factor
    }

    /// 获取验证者列表
    pub fn get_validators(&self) -> Vec<(Address, StakeAmount)> {
        self.validators.iter().map(|(addr, stake)| (*addr, *stake)).collect()
    }

    /// 获取验证者数量
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }
}

impl Default for VRFSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vrf_keypair_generation() {
        let key_pair = VRFKeyPair::generate();
        
        // 验证公钥可以从私钥派生
        let expected_public = RistrettoPoint::mul_base(&key_pair.private_key);
        assert_eq!(key_pair.public_key, expected_public);
        
        // 验证字节表示长度
        assert_eq!(key_pair.public_key_bytes().len(), 32);
        assert_eq!(key_pair.private_key_bytes().len(), 32);
    }

    #[test]
    fn test_vrf_keypair_from_seed() {
        let seed = b"test_seed_for_vrf";
        let key_pair1 = VRFKeyPair::from_seed(seed);
        let key_pair2 = VRFKeyPair::from_seed(seed);
        
        // 相同种子应该生成相同的密钥对
        assert_eq!(key_pair1.private_key, key_pair2.private_key);
        assert_eq!(key_pair1.public_key, key_pair2.public_key);
    }

    #[test]
    fn test_vrf_calculate_and_verify() {
        let key_pair = VRFKeyPair::generate();
        let message = b"Hello, VRF!";
        
        // 计算 VRF
        let output = VRFCalculator::calculate(&key_pair, message).unwrap();
        
        // 验证 VRF
        let verified = VRFCalculator::verify(&key_pair.public_key, message, &output).unwrap();
        assert!(verified);
        
        // 验证错误消息应该失败
        let wrong_message = b"Wrong message";
        let verified_wrong = VRFCalculator::verify(&key_pair.public_key, wrong_message, &output).unwrap();
        assert!(!verified_wrong);
    }

    #[test]
    fn test_vrf_proof_serialization() {
        let key_pair = VRFKeyPair::generate();
        let message = b"Serialization test";
        
        let output = VRFCalculator::calculate(&key_pair, message).unwrap();
        let proof_bytes = output.proof.to_bytes();
        
        // 反序列化
        let proof_restored = VRFProof::from_bytes(&proof_bytes).unwrap();
        
        // 验证证明相同
        assert_eq!(output.proof.gamma, proof_restored.gamma);
        assert_eq!(output.proof.challenge, proof_restored.challenge);
        assert_eq!(output.proof.response, proof_restored.response);
    }

    #[test]
    fn test_vrf_selector() {
        let mut selector = VRFSelector::new();
        
        // 添加验证者
        let addr1 = Address::from([1u8; 20]);
        let addr2 = Address::from([2u8; 20]);
        let key1 = VRFKeyPair::generate();
        let key2 = VRFKeyPair::generate();
        
        selector.add_validator(addr1, 1000, key1);
        selector.add_validator(addr2, 2000, key2);
        
        assert_eq!(selector.validator_count(), 2);
        
        // 选择提议者
        let message = b"selection_test";
        let (proposer, output) = selector.select_proposer(message, 1).unwrap();
        
        // 验证选择
        let verified = selector.verify_selection(proposer, message, 1, &output).unwrap();
        assert!(verified);
        
        // 验证应该是 addr1 或 addr2 之一
        assert!(proposer == addr1 || proposer == addr2);
    }

    #[test]
    fn test_vrf_deterministic_output() {
        let key_pair = VRFKeyPair::generate();
        let message = b"Deterministic test";
        
        let output1 = VRFCalculator::calculate(&key_pair, message).unwrap();
        let output2 = VRFCalculator::calculate(&key_pair, message).unwrap();
        
        // 相同密钥和消息应该产生相同的输出
        assert_eq!(output1.output, output2.output);
    }

    #[test]
    fn test_vrf_different_keys() {
        let key1 = VRFKeyPair::generate();
        let key2 = VRFKeyPair::generate();
        let message = b"Same message";
        
        let output1 = VRFCalculator::calculate(&key1, message).unwrap();
        let output2 = VRFCalculator::calculate(&key2, message).unwrap();
        
        // 不同密钥应该产生不同的输出
        assert_ne!(output1.output, output2.output);
    }

    #[test]
    fn test_vrf_score_calculation() {
        let vrf_output = [0u8; 32]; // 最小输出
        let stake = 1000;
        let total_stake = 10000;
        
        let score = VRFSelector::calculate_vrf_score(&vrf_output, stake, total_stake);
        
        // 最小 VRF 输出应该产生很小的分数
        assert!(score < 0.1);
        
        // 测试最大输出
        let max_output = [0xFFu8; 32];
        let max_score = VRFSelector::calculate_vrf_score(&max_output, stake, total_stake);
        
        // 最大输出应该产生较大的分数
        assert!(max_score > score);
    }
}