//! Configuration for the Solidity compiler

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the Solidity compiler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolcConfig {
    /// Path to the solc compiler binary (auto-detected if None)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solc_path: Option<PathBuf>,

    /// Minimum required solc version (semver constraint)
    #[serde(default = "default_min_version")]
    pub min_version: String,

    /// Target EVM version
    #[serde(default)]
    pub evm_version: EvmVersion,

    /// Optimization settings
    #[serde(default)]
    pub optimization: OptimizationSettings,

    /// Import remappings (e.g., "@openzeppelin=lib/openzeppelin-contracts/contracts")
    #[serde(default)]
    pub remappings: Vec<String>,

    /// Allowed paths for file imports (security)
    #[serde(default)]
    pub allow_paths: Vec<PathBuf>,

    /// Stop after parsing (for syntax checking only)
    #[serde(default)]
    pub parse_only: bool,

    /// Enable/disable compiler warnings
    #[serde(default = "default_warnings")]
    pub warnings: bool,

    /// Additional compiler arguments
    #[serde(default)]
    pub extra_args: Vec<String>,
}

impl Default for SolcConfig {
    fn default() -> Self {
        Self {
            solc_path: None,
            min_version: "0.8.0".to_string(),
            evm_version: EvmVersion::default(),
            optimization: OptimizationSettings::default(),
            remappings: Vec::new(),
            allow_paths: vec![PathBuf::from(".")],
            parse_only: false,
            warnings: true,
            extra_args: Vec::new(),
        }
    }
}

fn default_min_version() -> String {
    "0.8.0".to_string()
}

fn default_warnings() -> bool {
    true
}

/// EVM version target for compilation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EvmVersion {
    Homestead,
    TangerineWhistle,
    SpuriousDragon,
    Byzantium,
    Constantinople,
    Petersburg,
    Istanbul,
    Berlin,
    London,
    Paris,
    Shanghai,
    Cancun,
    Prague,
}

impl Default for EvmVersion {
    fn default() -> Self {
        Self::Paris
    }
}

impl std::fmt::Display for EvmVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvmVersion::Homestead => write!(f, "homestead"),
            EvmVersion::TangerineWhistle => write!(f, "tangerineWhistle"),
            EvmVersion::SpuriousDragon => write!(f, "spuriousDragon"),
            EvmVersion::Byzantium => write!(f, "byzantium"),
            EvmVersion::Constantinople => write!(f, "constantinople"),
            EvmVersion::Petersburg => write!(f, "petersburg"),
            EvmVersion::Istanbul => write!(f, "istanbul"),
            EvmVersion::Berlin => write!(f, "berlin"),
            EvmVersion::London => write!(f, "london"),
            EvmVersion::Paris => write!(f, "paris"),
            EvmVersion::Shanghai => write!(f, "shanghai"),
            EvmVersion::Cancun => write!(f, "cancun"),
            EvmVersion::Prague => write!(f, "prague"),
        }
    }
}

/// Solidity compiler optimization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationSettings {
    /// Enable optimizer
    #[serde(default)]
    pub enabled: bool,

    /// Number of optimization runs
    #[serde(default = "default_runs")]
    pub runs: u32,

    /// Detailed settings for newer solc versions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<OptimizationDetails>,
}

impl Default for OptimizationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            runs: 200,
            details: None,
        }
    }
}

fn default_runs() -> u32 {
    200
}

/// Detailed optimization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationDetails {
    /// Peephole optimization
    #[serde(default = "default_true")]
    pub peephole: bool,

    /// Inliner
    #[serde(default = "default_true")]
    pub inliner: bool,

    /// Jumpdest remover
    #[serde(default = "default_true")]
    pub jumpdest_remover: bool,

    /// Order literals
    #[serde(default = "default_true")]
    pub order_literals: bool,

    /// Deduplicate
    #[serde(default = "default_true")]
    pub deduplicate: bool,

    /// Cse
    #[serde(default = "default_true")]
    pub cse: bool,

    /// Constant optimizer
    #[serde(default = "default_true")]
    pub constant_optimizer: bool,

    /// Simple counter for loop unrolling
    #[serde(default = "default_true")]
    pub simple_counter_for_loop_unhandled_condition: bool,
}

fn default_true() -> bool {
    true
}
