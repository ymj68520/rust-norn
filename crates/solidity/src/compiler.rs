//! Solidity compiler service
//!
//! This module provides the `SolidityCompiler` struct that wraps the `solc`
//! command-line compiler. It discovers solc on the system, validates versions,
//! and compiles Solidity source code to EVM bytecode + ABI artifacts.
//!
//! # Dependencies
//!
//! Requires `solc` (Solidity compiler) installed on the system:
//! ```bash
//! pip install solc-select
//! solc-select install 0.8.20
//! solc-select use 0.8.20
//! ```

use crate::artifact::{CompilationError, CompileResult, CompiledContract, SolcOutput};
use crate::config::{EvmVersion, OptimizationSettings, SolcConfig};
use crate::error::{SolidityError, SolidityResult};
use regex::Regex;
use serde_json::json;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use tiny_keccak::{Hasher, Keccak};
use tracing::{debug, info, warn};

lazy_static::lazy_static! {
    /// Regex to extract version from solc --version output
    static ref VERSION_RE: Regex = Regex::new(r"(?i)version:\s*([0-9]+\.[0-9]+\.[0-9]+[^,\s]*)").unwrap();

    /// Regex to detect solc version strings
    static ref SOLC_VERSION_RE: Regex = Regex::new(r"([0-9]+\.[0-9]+\.[0-9]+)").unwrap();
}

/// Solidity compiler wrapper
///
/// This struct manages the `solc` compiler binary and provides methods
/// to compile Solidity source code into EVM bytecode and ABI definitions.
pub struct SolidityCompiler {
    config: SolcConfig,
    solc_path: PathBuf,
    detected_version: String,
}

impl SolidityCompiler {
    /// Create a new SolidityCompiler with the given configuration
    ///
    /// This will:
    /// 1. Find the solc binary (using configured path or auto-detection)
    /// 2. Verify the compiler version meets minimum requirements
    pub fn new(config: SolcConfig) -> SolidityResult<Self> {
        info!("Initializing Solidity compiler");

        // Find solc binary
        let solc_path = if let Some(ref path) = config.solc_path {
            path.clone()
        } else {
            Self::find_solc()?
        };

        debug!("Found solc at: {:?}", solc_path);

        // Verify it exists
        if !solc_path.exists() {
            return Err(SolidityError::SolcNotFound);
        }

        // Detect version
        let detected_version = Self::get_version(&solc_path)?;

        // Validate version against minimum requirement
        Self::validate_version(&detected_version, &config.min_version)?;

        info!(
            "Solidity compiler ready: version {} at {:?}",
            detected_version, solc_path
        );

        Ok(Self {
            config,
            solc_path,
            detected_version,
        })
    }

    /// Find the solc compiler binary
    ///
    /// Searches in the following order:
    /// 1. PATH environment variable (via `which`)
    /// 2. Common installation locations
    /// 3. solc-select managed versions
    pub fn find_solc() -> SolidityResult<PathBuf> {
        debug!("Searching for solc compiler");

        // Try to find using `which` crate
        match which::which("solc") {
            Ok(path) => {
                debug!("Found solc via which: {:?}", path);
                return Ok(path);
            }
            Err(_) => {
                debug!("solc not found in PATH");
            }
        }

        // Try common locations
        let common_paths = vec![
            "/usr/local/bin/solc",
            "/usr/bin/solc",
            "/opt/homebrew/bin/solc",
        ];

        for path_str in common_paths {
            let path = PathBuf::from(path_str);
            if path.exists() {
                debug!("Found solc at: {:?}", path);
                return Ok(path);
            }
        }

        // Try solc-select
        if let Ok(output) = Command::new("solc-select").arg("versions").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(latest) = stdout.lines().last() {
                    if let Some(ver) = latest.split_whitespace().next() {
                        let home = std::env::var("HOME").unwrap_or_default();
                        let select_path =
                            PathBuf::from(format!("{}/.solc-select/artifacts/{}/solc", home, ver));
                        if select_path.exists() {
                            return Ok(select_path);
                        }
                    }
                }
            }
        }

        Err(SolidityError::SolcNotFound)
    }

    /// Get the version string of the solc compiler
    pub fn get_version(solc_path: &Path) -> SolidityResult<String> {
        let output = Command::new(solc_path)
            .arg("--version")
            .output()
            .map_err(|_| SolidityError::SolcNotFound)?;

        if !output.status.success() {
            return Err(SolidityError::SolcNotFound);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        debug!("solc version output: {}", stdout);

        // Parse version from output:
        // solc, the solidity compiler commandline interface
        // Version: 0.8.20+commit.a1b2c3d
        if let Some(caps) = VERSION_RE.captures(&stdout) {
            if let Some(ver) = caps.get(1) {
                return Ok(ver.as_str().to_string());
            }
        }

        // Try to find any version-like string
        if let Some(caps) = SOLC_VERSION_RE.captures(&stdout) {
            if let Some(ver) = caps.get(1) {
                return Ok(ver.as_str().to_string());
            }
        }

        Err(SolidityError::ParseError {
            message: format!("Could not parse solc version from: {}", stdout),
        })
    }

    /// Validate that the detected version meets the minimum requirement
    fn validate_version(detected: &str, required: &str) -> SolidityResult<()> {
        debug!(
            "Validating version: detected={}, required={}",
            detected, required
        );

        let detected_ver = semver::Version::parse(detected.split('+').next().unwrap_or(detected))
            .map_err(|_| SolidityError::VersionMismatch {
            found: detected.to_string(),
            required: required.to_string(),
        })?;

        let required_ver =
            semver::Version::parse(required).map_err(|_| SolidityError::VersionMismatch {
                found: detected.to_string(),
                required: required.to_string(),
            })?;

        if detected_ver >= required_ver {
            Ok(())
        } else {
            Err(SolidityError::VersionMismatch {
                found: detected.to_string(),
                required: required.to_string(),
            })
        }
    }

    /// Get the detected compiler version
    pub fn version(&self) -> &str {
        &self.detected_version
    }

    /// Get the solc binary path
    pub fn solc_path(&self) -> &Path {
        &self.solc_path
    }

    /// Compile a Solidity source string
    ///
    /// # Arguments
    /// * `source` - The Solidity source code as a string
    /// * `contract_name` - Optional filter for a specific contract name
    ///
    /// # Returns
    /// A compilation result containing all compiled contracts
    pub fn compile_source(
        &self,
        source: &str,
        contract_name: Option<&str>,
    ) -> SolidityResult<CompileResult> {
        info!("Compiling Solidity source (length={} bytes)", source.len());
        debug!("Source preview: {}", &source[..source.len().min(200)]);

        // Write source to temporary file
        let temp_dir = tempfile::tempdir()?;
        let source_path = temp_dir.path().join("input.sol");
        std::fs::write(&source_path, source)?;

        self.compile_files(&[source_path], contract_name)
    }

    /// Compile Solidity source files
    ///
    /// # Arguments
    /// * `files` - Paths to .sol files to compile
    /// * `contract_name` - Optional filter for a specific contract name
    ///
    /// # Returns
    /// A compilation result containing all compiled contracts
    pub fn compile_files(
        &self,
        files: &[PathBuf],
        contract_name: Option<&str>,
    ) -> SolidityResult<CompileResult> {
        if files.is_empty() {
            return Err(SolidityError::ConfigError {
                message: "No source files provided for compilation".to_string(),
            });
        }

        info!("Compiling {} Solidity files", files.len());

        // Build standard JSON input
        let json_input = self.build_standard_json(files)?;

        // Create temporary directory for input/output
        let temp_dir = tempfile::tempdir()?;
        let input_path = temp_dir.path().join("input.json");
        let output_path = temp_dir.path().join("output.json");

        std::fs::write(&input_path, serde_json::to_string_pretty(&json_input)?)?;

        debug!(
            "Invoking solc: {} --standard-json {}",
            self.solc_path.display(),
            input_path.display()
        );

        // Invoke solc with standard JSON
        let output = Command::new(&self.solc_path)
            .arg("--standard-json")
            .arg(&input_path)
            .output()
            .map_err(|e| SolidityError::IoError {
                message: format!("Failed to execute solc: {}", e),
            })?;

        // Read output from stdout or output file
        let output_json = if output_path.exists() {
            std::fs::read_to_string(&output_path)?
        } else if !output.stdout.is_empty() {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            String::from_utf8_lossy(&output.stderr).to_string()
        };

        debug!(
            "solc exit status: {}, output length: {}",
            output.status,
            output_json.len()
        );

        // Parse the output
        let solc_output: SolcOutput =
            serde_json::from_str(&output_json).map_err(|e| SolidityError::ParseError {
                message: format!(
                    "Failed to parse solc output: {}. Output: {}",
                    e,
                    &output_json[..output_json.len().min(500)]
                ),
            })?;

        // Process compilation errors
        let actual_errors: Vec<CompilationError> = solc_output
            .errors
            .iter()
            .filter(|e| {
                e.error_type == "TypeError"
                    || e.error_type == "ParserError"
                    || e.error_type == "SemanticError"
                    || e.error_type == "SyntaxError"
                    || e.error_type == "DeclarationError"
                    || (e.error_type == "Warning"
                        && (e.message.contains("error")
                            || e.message.contains("unused")
                            || e.component == "general"))
            })
            .cloned()
            .collect();

        if !actual_errors.is_empty() {
            let error_messages: Vec<String> = actual_errors
                .iter()
                .map(|e| format!("{}: {}", e.error_type, e.message))
                .collect();

            warn!(
                "Compilation completed with {} errors: {:?}",
                actual_errors.len(),
                error_messages
            );

            return Err(SolidityError::CompilationErrors {
                errors: actual_errors,
            });
        }

        // Extract compiled contracts from output
        let mut contracts: Vec<CompiledContract> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();

        for (_source_path, contracts_map) in &solc_output.contracts {
            for contract in contracts_map.values() {
                if let Some(filter) = contract_name {
                    if contract.contract_name != filter {
                        continue;
                    }
                }

                contracts.push(contract.clone());
            }
        }

        // Collect warnings
        for err in &solc_output.errors {
            if err.error_type == "Warning" {
                warnings.push(err.message.clone());
            }
        }

        info!(
            "Compilation complete: {} contracts, {} warnings",
            contracts.len(),
            warnings.len()
        );

        Ok(CompileResult {
            contracts,
            warnings,
            errors: Vec::new(),
        })
    }

    /// Build solc standard JSON input
    fn build_standard_json(&self, files: &[PathBuf]) -> SolidityResult<Value> {
        let settings = json!({
            "optimizer": {
                "enabled": self.config.optimization.enabled,
                "runs": self.config.optimization.runs,
            },
            "outputSelection": {
                "*": {
                    "*": [
                        "abi",
                        "evm.bytecode",
                        "evm.bytecode.sourceMap",
                        "evm.deployedBytecode",
                        "evm.deployedBytecode.sourceMap",
                        "devdoc",
                        "userdoc",
                        "metadata",
                        "methodIdentifiers",
                    ]
                }
            },
            "evmVersion": self.config.evm_version.to_string(),
            "remappings": self.config.remappings,
        });

        let mut input = json!({
            "language": "Solidity",
            "sources": {},
            "settings": settings,
        });

        // Add source files
        if let Some(obj) = input.as_object_mut() {
            if let Some(sources) = obj.get_mut("sources") {
                if let Some(sources_map) = sources.as_object_mut() {
                    for file_path in files {
                        let key = file_path.display().to_string();
                        let content = std::fs::read_to_string(file_path)?;
                        sources_map.insert(key, json!({"content": content}));
                    }
                }
            }
        }

        Ok(input)
    }
}

impl std::fmt::Debug for SolidityCompiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolidityCompiler")
            .field("solc_path", &self.solc_path)
            .field("detected_version", &self.detected_version)
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        let output = "solc, the solidity compiler commandline interface\nVersion: 0.8.20+commit.a1b2c3d.Linux.gcc\n";
        assert!(VERSION_RE.is_match(output));
        if let Some(caps) = VERSION_RE.captures(output) {
            assert!(caps.get(1).unwrap().as_str().starts_with("0.8.20"));
        }
    }

    #[test]
    fn test_evm_version_display() {
        assert_eq!(EvmVersion::Paris.to_string(), "paris");
        assert_eq!(EvmVersion::London.to_string(), "london");
        assert_eq!(EvmVersion::Cancun.to_string(), "cancun");
    }
}
