//! ABI (Application Binary Interface) encoding/decoding
//!
//! This module implements Ethereum's ABI for encoding and decoding function calls,
//! events, and data types according to the Contract ABI Specification.
//!
//! See: https://docs.soliditylang.org/en/develop/abi-spec.html

use crate::evm::{EVMError, EVMResult};
use norn_common::types::Address;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// ABI encoding/decoder
pub struct ABI;

impl ABI {
    /// Encode a function call with parameters
    ///
    /// # Arguments
    /// * `function_signature` - Function signature (e.g., "transfer(address,uint256)")
    /// * `params` - Parameters to encode
    ///
    /// # Returns
    /// Encoded function call data
    pub fn encode_function_call(
        function_signature: &str,
        params: &[ABIParam],
    ) -> EVMResult<Vec<u8>> {
        // Calculate function selector (first 4 bytes of Keccak256 hash)
        let selector = Self::function_selector(function_signature);

        // Encode parameters
        let mut encoded_params = Vec::new();
        for param in params {
            encoded_params.extend_from_slice(&Self::encode_param(param)?);
        }

        // Combine selector and encoded parameters
        let mut result = Vec::with_capacity(4 + encoded_params.len());
        result.extend_from_slice(&selector);
        result.extend_from_slice(&encoded_params);

        Ok(result)
    }

    /// Decode function return values
    ///
    /// # Arguments
    /// * `data` - Return data to decode
    /// * `types` - Expected types
    ///
    /// # Returns
    /// Decoded parameters
    pub fn decode_function_return(
        data: &[u8],
        types: &[ABIType],
    ) -> EVMResult<Vec<ABIParam>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        let mut params = Vec::new();
        let mut offset = 0;

        for ty in types {
            let (param, new_offset) = Self::decode_param(data, offset, ty)?;
            params.push(param);
            offset = new_offset;
        }

        Ok(params)
    }

    /// Encode an event log
    ///
    /// # Arguments
    /// * `event_signature` - Event signature (e.g., "Transfer(address,address,uint256)")
    /// * `topics` - Indexed parameters (up to 3)
    /// * `data` - Non-indexed parameters
    ///
    /// # Returns
    /// Event topics and data
    pub fn encode_event(
        event_signature: &str,
        topics: &[ABIParam],
        data: &[ABIParam],
    ) -> EVMResult<(Vec<[u8; 32]>, Vec<u8>)> {
        // Calculate event signature hash (first topic)
        let mut result_topics = vec![Self::event_signature_hash(event_signature)];

        // Encode indexed parameters as topics
        for topic in topics {
            result_topics.push(Self::encode_topic(topic)?);
        }

        // Encode non-indexed parameters as data
        let encoded_data = Self::encode_params(data)?;

        Ok((result_topics, encoded_data))
    }

    /// Decode an event log
    ///
    /// # Arguments
    /// * `topics` - Event topics
    /// * `data` - Event data
    /// * `indexed_types` - Types of indexed parameters
    /// * `non_indexed_types` - Types of non-indexed parameters
    ///
    /// # Returns
    /// Decoded event parameters
    pub fn decode_event(
        topics: &[[u8; 32]],
        data: &[u8],
        indexed_types: &[ABIType],
        non_indexed_types: &[ABIType],
    ) -> EVMResult<(Vec<ABIParam>, Vec<ABIParam>)> {
        if topics.is_empty() {
            return Err(EVMError::Execution("Event has no topics".to_string()));
        }

        // Skip first topic (event signature)
        let mut indexed_params = Vec::new();
        for (i, ty) in indexed_types.iter().enumerate() {
            let topic = topics.get(i + 1)
                .ok_or_else(|| EVMError::Execution(format!("Missing topic at index {}", i)))?;
            indexed_params.push(Self::decode_topic(topic, ty)?);
        }

        // Decode non-indexed parameters from data
        let non_indexed_params = Self::decode_params(data, non_indexed_types)?;

        Ok((indexed_params, non_indexed_params))
    }

    /// Calculate function selector (first 4 bytes of signature hash)
    pub fn function_selector(signature: &str) -> [u8; 4] {
        let hash = Self::keccak256(signature.as_bytes());
        let mut selector = [0u8; 4];
        selector.copy_from_slice(&hash[0..4]);
        selector
    }

    /// Calculate event signature hash
    pub fn event_signature_hash(signature: &str) -> [u8; 32] {
        Self::keccak256(signature.as_bytes())
    }

    /// Encode a single parameter
    fn encode_param(param: &ABIParam) -> EVMResult<Vec<u8>> {
        match &param.value {
            ABIValue::Uint(uint_value, size) => {
                Self::encode_uint(*uint_value, *size)
            }
            ABIValue::Int(int_value, size) => {
                Self::encode_int(*int_value, *size)
            }
            ABIValue::Address(address) => {
                Ok(Self::encode_address(address))
            }
            ABIValue::Bool(b) => {
                Ok(Self::encode_bool(*b))
            }
            ABIValue::Bytes(bytes) => {
                Self::encode_bytes(bytes)
            }
            ABIValue::String(s) => {
                Self::encode_string(s)
            }
            ABIValue::Array(params) => {
                Self::encode_array(params)
            }
            ABIValue::FixedArray(params) => {
                Self::encode_fixed_array(params)
            }
            ABIValue::Tuple(fields) => {
                Self::encode_tuple(fields)
            }
        }
    }

    /// Decode a single parameter
    fn decode_param(data: &[u8], offset: usize, ty: &ABIType) -> EVMResult<(ABIParam, usize)> {
        match ty {
            ABIType::Uint(size) => {
                let value = Self::decode_uint(data, offset, *size)?;
                Ok((ABIParam::new(ABIValue::Uint(value, *size)), offset + 32))
            }
            ABIType::Int(size) => {
                let value = Self::decode_int(data, offset, *size)?;
                Ok((ABIParam::new(ABIValue::Int(value, *size)), offset + 32))
            }
            ABIType::Address => {
                let address = Self::decode_address(data, offset)?;
                Ok((ABIParam::new(ABIValue::Address(address)), offset + 32))
            }
            ABIType::Bool => {
                let b = Self::decode_bool(data, offset)?;
                Ok((ABIParam::new(ABIValue::Bool(b)), offset + 32))
            }
            ABIType::Bytes => {
                let (bytes, new_offset) = Self::decode_bytes(data, offset)?;
                Ok((ABIParam::new(ABIValue::Bytes(bytes)), new_offset))
            }
            ABIType::String => {
                let (s, new_offset) = Self::decode_string(data, offset)?;
                Ok((ABIParam::new(ABIValue::String(s)), new_offset))
            }
            _ => {
                Err(EVMError::Execution(format!("Unsupported type: {:?}", ty)))
            }
        }
    }

    /// Encode multiple parameters
    fn encode_params(params: &[ABIParam]) -> EVMResult<Vec<u8>> {
        let mut encoded = Vec::new();
        let mut head_offset = params.len() * 32; // Start after head section

        // First pass: encode static parameters and calculate offsets for dynamic
        for param in params {
            if Self::is_dynamic_type(param) {
                // Encode offset
                encoded.extend_from_slice(&Self::encode_uint(head_offset as u64, 256)?);
                // Calculate new offset (will be updated in second pass)
            } else {
                // Encode static parameter directly
                encoded.extend_from_slice(&Self::encode_param(param)?);
            }
        }

        // Second pass: encode dynamic parameters
        for param in params {
            if Self::is_dynamic_type(param) {
                let param_data = Self::encode_param(param)?;
                encoded.extend_from_slice(&param_data);
            }
        }

        Ok(encoded)
    }

    /// Decode multiple parameters
    fn decode_params(data: &[u8], types: &[ABIType]) -> EVMResult<Vec<ABIParam>> {
        let mut params = Vec::new();
        let mut offset = 0;

        for ty in types {
            let (param, new_offset) = Self::decode_param(data, offset, ty)?;
            params.push(param);
            offset = new_offset;
        }

        Ok(params)
    }

    /// Encode a uint value
    fn encode_uint(value: u64, size: u16) -> EVMResult<Vec<u8>> {
        let bytes = (size / 8) as usize;
        let mut encoded = vec![0u8; 32];
        let value_bytes = value.to_be_bytes();
        let start = 32 - bytes.min(8);
        encoded[start..].copy_from_slice(&value_bytes[..bytes.min(8)]);
        Ok(encoded)
    }

    /// Decode a uint value
    fn decode_uint(data: &[u8], offset: usize, size: u16) -> EVMResult<u64> {
        let end = offset + 32;
        if end > data.len() {
            return Err(EVMError::Execution("Insufficient data for uint".to_string()));
        }

        let bytes = (size / 8) as usize;
        let start = 32 - bytes.min(8);
        let slice = &data[offset + start..offset + 32];
        let value = u64::from_be_bytes(
            slice[..8].try_into()
                .map_err(|_| EVMError::Execution("Invalid uint encoding".to_string()))?
        );
        Ok(value)
    }

    /// Encode an int value
    fn encode_int(value: i64, size: u16) -> EVMResult<Vec<u8>> {
        let bytes = (size / 8) as usize;
        let mut encoded = if value < 0 {
            vec![0xFFu8; 32]
        } else {
            vec![0u8; 32]
        };

        let value_bytes = value.to_be_bytes();
        let start = 32 - bytes.min(8);
        encoded[start..].copy_from_slice(&value_bytes[..bytes.min(8)]);
        Ok(encoded)
    }

    /// Decode an int value
    fn decode_int(data: &[u8], offset: usize, size: u16) -> EVMResult<i64> {
        let end = offset + 32;
        if end > data.len() {
            return Err(EVMError::Execution("Insufficient data for int".to_string()));
        }

        let slice = &data[offset..end];
        let value = i64::from_be_bytes(
            slice[24..32].try_into()
                .map_err(|_| EVMError::Execution("Invalid int encoding".to_string()))?
        );
        Ok(value)
    }

    /// Encode an address
    fn encode_address(address: &Address) -> Vec<u8> {
        let mut encoded = vec![0u8; 32];
        encoded[12..32].copy_from_slice(&address.0);
        encoded
    }

    /// Decode an address
    fn decode_address(data: &[u8], offset: usize) -> EVMResult<Address> {
        let end = offset + 32;
        if end > data.len() {
            return Err(EVMError::Execution("Insufficient data for address".to_string()));
        }

        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&data[offset + 12..end]);
        Ok(Address(addr_bytes))
    }

    /// Encode a boolean
    fn encode_bool(value: bool) -> Vec<u8> {
        let mut encoded = vec![0u8; 32];
        encoded[31] = if value { 1 } else { 0 };
        encoded
    }

    /// Decode a boolean
    fn decode_bool(data: &[u8], offset: usize) -> EVMResult<bool> {
        let end = offset + 32;
        if end > data.len() {
            return Err(EVMError::Execution("Insufficient data for bool".to_string()));
        }

        Ok(data[end - 1] != 0)
    }

    /// Encode bytes (dynamic)
    fn encode_bytes(bytes: &[u8]) -> EVMResult<Vec<u8>> {
        let mut encoded = Self::encode_uint(bytes.len() as u64, 256)?;
        encoded.extend_from_slice(bytes);

        // Pad to multiple of 32
        while encoded.len() % 32 != 0 {
            encoded.push(0);
        }

        Ok(encoded)
    }

    /// Decode bytes (dynamic)
    fn decode_bytes(data: &[u8], offset: usize) -> EVMResult<(Vec<u8>, usize)> {
        // Read length
        let len = Self::decode_uint(data, offset, 256)? as usize;
        let start = offset + 32;

        // Read bytes
        let end = start + len;
        if end > data.len() {
            return Err(EVMError::Execution("Insufficient data for bytes".to_string()));
        }

        let bytes = data[start..end].to_vec();

        // Calculate new offset (padded to 32-byte boundary)
        let padded_len = ((len + 31) / 32) * 32;
        let new_offset = start + padded_len;

        Ok((bytes, new_offset))
    }

    /// Encode a string
    fn encode_string(s: &str) -> EVMResult<Vec<u8>> {
        Self::encode_bytes(s.as_bytes())
    }

    /// Decode a string
    fn decode_string(data: &[u8], offset: usize) -> EVMResult<(String, usize)> {
        let (bytes, new_offset) = Self::decode_bytes(data, offset)?;
        let s = String::from_utf8(bytes)
            .map_err(|_| EVMError::Execution("Invalid UTF-8 in string".to_string()))?;
        Ok((s, new_offset))
    }

    /// Encode an array
    fn encode_array(params: &[ABIParam]) -> EVMResult<Vec<u8>> {
        let mut encoded = Self::encode_uint(params.len() as u64, 256)?;

        for param in params {
            encoded.extend_from_slice(&Self::encode_param(param)?);
        }

        Ok(encoded)
    }

    /// Decode an array
    fn decode_array(_data: &[u8], _offset: usize, _ty: &ABIType) -> EVMResult<(Vec<ABIParam>, usize)> {
        // TODO: Implement array decoding
        Err(EVMError::Execution("Array decoding not yet implemented".to_string()))
    }

    /// Encode a fixed-size array
    fn encode_fixed_array(params: &[ABIParam]) -> EVMResult<Vec<u8>> {
        let mut encoded = Vec::new();
        for param in params {
            encoded.extend_from_slice(&Self::encode_param(param)?);
        }
        Ok(encoded)
    }

    /// Encode a tuple
    fn encode_tuple(fields: &[(String, ABIParam)]) -> EVMResult<Vec<u8>> {
        let params: Vec<ABIParam> = fields.iter().map(|(_, p)| p.clone()).collect();
        Self::encode_params(&params)
    }

    /// Encode a parameter as a topic
    fn encode_topic(param: &ABIParam) -> EVMResult<[u8; 32]> {
        let encoded = Self::encode_param(param)?;

        if encoded.len() != 32 {
            return Err(EVMError::Execution(format!(
                "Topic parameter must be 32 bytes, got {}",
                encoded.len()
            )));
        }

        let mut topic = [0u8; 32];
        topic.copy_from_slice(&encoded);
        Ok(topic)
    }

    /// Decode a parameter from a topic
    fn decode_topic(topic: &[u8; 32], ty: &ABIType) -> EVMResult<ABIParam> {
        Self::decode_param(topic, 0, ty).map(|(p, _)| p)
    }

    /// Check if a parameter type is dynamic
    fn is_dynamic_type(param: &ABIParam) -> bool {
        matches!(&param.value, ABIValue::Bytes(_) | ABIValue::String(_) | ABIValue::Array(_))
    }

    /// Compute Keccak256 hash
    fn keccak256(data: &[u8]) -> [u8; 32] {
        use tiny_keccak::{Hasher, Keccak};
        let mut hasher = Keccak::v256();
        let mut output = [0u8; 32];
        hasher.update(data);
        hasher.finalize(&mut output);
        output
    }
}

/// ABI parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ABIParam {
    /// Parameter name (optional)
    pub name: Option<String>,

    /// Parameter value
    pub value: ABIValue,
}

impl ABIParam {
    /// Create a new ABI parameter
    pub fn new(value: ABIValue) -> Self {
        Self {
            name: None,
            value,
        }
    }

    /// Create a named ABI parameter
    pub fn with_name(name: String, value: ABIValue) -> Self {
        Self {
            name: Some(name),
            value,
        }
    }
}

/// ABI value types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ABIValue {
    /// Unsigned integer (uint8, uint16, ..., uint256)
    Uint(u64, u16), // value, size in bits

    /// Signed integer (int8, int16, ..., int256)
    Int(i64, u16), // value, size in bits

    /// Address (20 bytes)
    Address(Address),

    /// Boolean
    Bool(bool),

    /// Dynamic bytes
    Bytes(Vec<u8>),

    /// String
    String(String),

    /// Array
    Array(Vec<ABIParam>),

    /// Fixed-size array
    FixedArray(Vec<ABIParam>),

    /// Tuple/struct
    Tuple(Vec<(String, ABIParam)>),
}

/// ABI type specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ABIType {
    /// Unsigned integer
    Uint(u16),

    /// Signed integer
    Int(u16),

    /// Address
    Address,

    /// Boolean
    Bool,

    /// Dynamic bytes
    Bytes,

    /// Fixed-size bytes
    FixedBytes(u8),

    /// String
    String,

    /// Array
    Array(Box<ABIType>),

    /// Fixed-size array
    FixedArray(Box<ABIType>, usize),

    /// Tuple
    Tuple(Vec<ABIType>),
}

/// Human-Readable ABI (for simplicity)
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct HumanReadableABI(Vec<String>);

impl HumanReadableABI {
    /// Create a new empty HumanReadableABI
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Add an item to the ABI
    pub fn push(&mut self, item: String) {
        self.0.push(item);
    }

    /// Parse a human-readable ABI into structured types
    pub fn parse(&self) -> EVMResult<Vec<ABIItem>> {
        let mut items = Vec::new();

        for item_str in &self.0 {
            let item = Self::parse_item(item_str)?;
            items.push(item);
        }

        Ok(items)
    }

    /// Parse a single ABI item
    pub fn parse_item(s: &str) -> EVMResult<ABIItem> {
        let s = s.trim();

        if s.starts_with("function ") {
            Self::parse_function(s)
        } else if s.starts_with("event ") {
            Self::parse_event(s)
        } else if s.starts_with("constructor ") {
            Self::parse_constructor(s)
        } else if s.starts_with("error ") {
            Self::parse_error(s)
        } else if s.starts_with("fallback") || s.starts_with("receive") {
            Self::parse_fallback(s)
        } else {
            Err(EVMError::Execution(format!("Unknown ABI item: {}", s)))
        }
    }

    /// Parse a function declaration
    fn parse_function(s: &str) -> EVMResult<ABIItem> {
        // Remove "function " prefix
        let s = s.strip_prefix("function ")
            .ok_or_else(|| EVMError::Execution("Missing function prefix".to_string()))?;

        // Split at '('
        let parts: Vec<&str> = s.split('(').collect();
        if parts.len() < 2 {
            return Err(EVMError::Execution("Invalid function syntax".to_string()));
        }

        let name = parts[0].trim().to_string();
        let params_str = parts[1].trim().trim_end_matches(')');

        // Parse parameters
        let inputs = if params_str.is_empty() {
            Vec::new()
        } else {
            params_str.split(',')
                .map(|p| Self::parse_param(p.trim()))
                .collect::<EVMResult<Vec<_>>>()?
        };

        // Parse return type (if present)
        let outputs = if s.contains("returns") {
            let returns_str = s.split("returns").nth(1)
                .ok_or_else(|| EVMError::Execution("Invalid returns syntax".to_string()))?
                .trim()
                .trim_start_matches('(')
                .trim_end_matches(')');

            if returns_str.is_empty() {
                Vec::new()
            } else {
                returns_str.split(',')
                    .map(|p| Self::parse_param(p.trim()))
                    .collect::<EVMResult<Vec<_>>>()?
            }
        } else {
            Vec::new()
        };

        Ok(ABIItem::Function {
            name,
            inputs,
            outputs,
        })
    }

    /// Parse an event declaration
    fn parse_event(s: &str) -> EVMResult<ABIItem> {
        let s = s.strip_prefix("event ")
            .ok_or_else(|| EVMError::Execution("Missing event prefix".to_string()))?;

        let parts: Vec<&str> = s.split('(').collect();
        if parts.len() < 2 {
            return Err(EVMError::Execution("Invalid event syntax".to_string()));
        }

        let name = parts[0].trim().to_string();
        let params_str = parts[1].trim().trim_end_matches(')');

        let inputs = if params_str.is_empty() {
            Vec::new()
        } else {
            params_str.split(',')
                .map(|p| Self::parse_event_param(p.trim()))
                .collect::<EVMResult<Vec<_>>>()?
        };

        Ok(ABIItem::Event {
            name,
            inputs,
        })
    }

    /// Parse a constructor declaration
    fn parse_constructor(s: &str) -> EVMResult<ABIItem> {
        let s = s.strip_prefix("constructor")
            .ok_or_else(|| EVMError::Execution("Missing constructor prefix".to_string()))?;

        let params_str = s.trim()
            .trim_start_matches('(')
            .trim_end_matches(')');

        let inputs = if params_str.is_empty() {
            Vec::new()
        } else {
            params_str.split(',')
                .map(|p| Self::parse_param(p.trim()))
                .collect::<EVMResult<Vec<_>>>()?
        };

        Ok(ABIItem::Constructor { inputs })
    }

    /// Parse an error declaration
    fn parse_error(s: &str) -> EVMResult<ABIItem> {
        let s = s.strip_prefix("error ")
            .ok_or_else(|| EVMError::Execution("Missing error prefix".to_string()))?;

        let parts: Vec<&str> = s.split('(').collect();
        if parts.len() < 2 {
            return Err(EVMError::Execution("Invalid error syntax".to_string()));
        }

        let name = parts[0].trim().to_string();
        let params_str = parts[1].trim().trim_end_matches(')');

        let inputs = if params_str.is_empty() {
            Vec::new()
        } else {
            params_str.split(',')
                .map(|p| Self::parse_param(p.trim()))
                .collect::<EVMResult<Vec<_>>>()?
        };

        Ok(ABIItem::Error {
            name,
            inputs,
        })
    }

    /// Parse fallback/receive function
    fn parse_fallback(s: &str) -> EVMResult<ABIItem> {
        if s.contains("fallback") {
            Ok(ABIItem::Fallback)
        } else if s.contains("receive") {
            Ok(ABIItem::Receive)
        } else {
            Err(EVMError::Execution("Invalid fallback/receive syntax".to_string()))
        }
    }

    /// Parse a parameter
    fn parse_param(s: &str) -> EVMResult<ABIParamType> {
        let parts: Vec<&str> = s.split_whitespace().collect();

        if parts.is_empty() {
            return Err(EVMError::Execution("Empty parameter".to_string()));
        }

        // First part is always the type
        let ty_str = parts[0];

        // Check if there's a parameter name (second part)
        let name = if parts.len() >= 2 {
            Some(parts[1].to_string())
        } else {
            None
        };

        let ty = Self::parse_type(ty_str)?;

        Ok(ABIParamType {
            name,
            ty,
            indexed: false,
        })
    }

    /// Parse an event parameter
    fn parse_event_param(s: &str) -> EVMResult<ABIParamType> {
        let parts: Vec<&str> = s.split_whitespace().collect();

        if parts.len() < 1 {
            return Err(EVMError::Execution("Empty event parameter".to_string()));
        }

        let mut indexed = false;
        let mut name = None;
        let mut ty_str = parts[0];

        // Check for "indexed" keyword
        if parts.len() >= 2 && parts[1] == "indexed" {
            indexed = true;
            if parts.len() >= 3 {
                name = Some(parts[2].to_string());
            }
        } else if parts.len() >= 2 {
            name = Some(parts[1].to_string());
        }

        let ty = Self::parse_type(ty_str)?;

        Ok(ABIParamType {
            name,
            ty,
            indexed,
        })
    }

    /// Parse a type string
    fn parse_type(s: &str) -> EVMResult<ABIType> {
        let s = s.trim();

        // Uint
        if let Some(size_str) = s.strip_prefix("uint") {
            if s == "uint" {
                return Ok(ABIType::Uint(256));
            }
            let size: u16 = size_str.parse()
                .map_err(|_| EVMError::Execution(format!("Invalid uint size: {}", s)))?;
            if size % 8 != 0 || size == 0 || size > 256 {
                return Err(EVMError::Execution(format!("Invalid uint size: {}", size)));
            }
            return Ok(ABIType::Uint(size));
        }

        // Int
        if let Some(size_str) = s.strip_prefix("int") {
            if s == "int" {
                return Ok(ABIType::Int(256));
            }
            let size: u16 = size_str.parse()
                .map_err(|_| EVMError::Execution(format!("Invalid int size: {}", s)))?;
            if size % 8 != 0 || size == 0 || size > 256 {
                return Err(EVMError::Execution(format!("Invalid int size: {}", size)));
            }
            return Ok(ABIType::Int(size));
        }

        // Address
        if s == "address" {
            return Ok(ABIType::Address);
        }

        // Bool
        if s == "bool" {
            return Ok(ABIType::Bool);
        }

        // Bytes
        if s == "bytes" {
            return Ok(ABIType::Bytes);
        }

        if let Some(size_str) = s.strip_prefix("bytes") {
            let size: u8 = size_str.parse()
                .map_err(|_| EVMError::Execution(format!("Invalid bytes size: {}", s)))?;
            if size == 0 || size > 32 {
                return Err(EVMError::Execution(format!("Invalid bytes size: {}", size)));
            }
            return Ok(ABIType::FixedBytes(size));
        }

        // String
        if s == "string" {
            return Ok(ABIType::String);
        }

        // Array
        if let Some(inner_str) = s.strip_suffix("[]") {
            let inner = Self::parse_type(inner_str)?;
            return Ok(ABIType::Array(Box::new(inner)));
        }

        Err(EVMError::Execution(format!("Unknown type: {}", s)))
    }
}

/// ABI item (function, event, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ABIItem {
    /// Function
    Function {
        name: String,
        inputs: Vec<ABIParamType>,
        outputs: Vec<ABIParamType>,
    },

    /// Event
    Event {
        name: String,
        inputs: Vec<ABIParamType>,
    },

    /// Constructor
    Constructor {
        inputs: Vec<ABIParamType>,
    },

    /// Error
    Error {
        name: String,
        inputs: Vec<ABIParamType>,
    },

    /// Fallback function
    Fallback,

    /// Receive function
    Receive,
}

/// ABI parameter type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ABIParamType {
    /// Parameter name (optional)
    pub name: Option<String>,

    /// Parameter type
    pub ty: ABIType,

    /// Whether this parameter is indexed (for events)
    pub indexed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_common::types::Hash;

    #[test]
    fn test_function_selector() {
        let selector = ABI::function_selector("transfer(address,uint256)");
        assert_eq!(selector.len(), 4);
    }

    #[test]
    fn test_encode_uint() {
        let encoded = ABI::encode_uint(42, 256).unwrap();
        assert_eq!(encoded.len(), 32);
        assert_eq!(encoded[31], 42);
    }

    #[test]
    fn test_encode_address() {
        let address = Address([1u8; 20]);
        let encoded = ABI::encode_address(&address);
        assert_eq!(encoded.len(), 32);
        assert_eq!(&encoded[12..32], &address.0[..]);
    }

    #[test]
    fn test_encode_bool() {
        let encoded_true = ABI::encode_bool(true);
        assert_eq!(encoded_true[31], 1);

        let encoded_false = ABI::encode_bool(false);
        assert_eq!(encoded_false[31], 0);
    }

    #[test]
    fn test_encode_function_call() {
        // Encode transfer(address,uint256)
        let to = Address([1u8; 20]);
        let amount = ABIParam::new(ABIValue::Uint(1000, 256));

        let encoded = ABI::encode_function_call(
            "transfer(address,uint256)",
            &[ABIParam::new(ABIValue::Address(to)), amount]
        ).unwrap();

        assert!(encoded.len() >= 4); // At least selector
    }

    #[test]
    fn test_parse_simple_type() {
        assert_eq!(HumanReadableABI::parse_type("uint256").unwrap(), ABIType::Uint(256));
        assert_eq!(HumanReadableABI::parse_type("address").unwrap(), ABIType::Address);
        assert_eq!(HumanReadableABI::parse_type("bool").unwrap(), ABIType::Bool);
        assert_eq!(HumanReadableABI::parse_type("bytes").unwrap(), ABIType::Bytes);
        assert_eq!(HumanReadableABI::parse_type("string").unwrap(), ABIType::String);
    }

    #[test]
    fn test_parse_function() {
        let func = "function transfer(address to, uint256 amount) returns (bool)";
        let item = HumanReadableABI::parse_item(func).unwrap();

        match item {
            ABIItem::Function { name, inputs, outputs } => {
                assert_eq!(name, "transfer");
                assert_eq!(inputs.len(), 2);
                assert_eq!(outputs.len(), 1);
            }
            _ => panic!("Expected function item"),
        }
    }

    #[test]
    fn test_parse_event() {
        let event = "event Transfer(address indexed from, address indexed to, uint256 value)";
        let item = HumanReadableABI::parse_item(event).unwrap();

        match item {
            ABIItem::Event { name, inputs } => {
                assert_eq!(name, "Transfer");
                assert_eq!(inputs.len(), 3);
                assert!(inputs[0].indexed);
                assert!(inputs[1].indexed);
                assert!(!inputs[2].indexed);
            }
            _ => panic!("Expected event item"),
        }
    }

    #[test]
    fn test_encode_event() {
        let from = Address([1u8; 20]);
        let to = Address([2u8; 20]);
        let value = ABIParam::new(ABIValue::Uint(1000, 256));

        let (topics, data) = ABI::encode_event(
            "Transfer(address,address,uint256)",
            &[
                ABIParam::new(ABIValue::Address(from)),
                ABIParam::new(ABIValue::Address(to)),
            ],
            &[value],
        ).unwrap();

        assert_eq!(topics.len(), 3); // signature + 2 indexed
        assert!(!data.is_empty());
    }

    #[test]
    fn test_keccak256() {
        let hash = ABI::keccak256(b"test");
        assert_eq!(hash.len(), 32);
    }
}

