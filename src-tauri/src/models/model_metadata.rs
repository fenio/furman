use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TensorInfo {
    pub name: String,
    pub dtype: String,
    pub shape: Vec<u64>,
    pub size_bytes: u64,
    pub param_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub format: String,
    pub file_size: u64,
    pub model_name: Option<String>,
    pub architecture: Option<String>,
    pub description: Option<String>,
    pub quantization: Option<String>,
    pub tensor_count: u64,
    pub total_parameters: u64,
    pub total_tensor_bytes: u64,
    // GGUF-specific
    pub context_length: Option<u64>,
    pub embedding_size: Option<u64>,
    pub block_count: Option<u64>,
    pub head_count: Option<u64>,
    pub head_count_kv: Option<u64>,
    pub vocab_size: Option<u64>,
    pub gguf_version: Option<u32>,
    pub byte_order: Option<String>,
    // ONNX-specific
    pub ir_version: Option<u64>,
    pub producer_name: Option<String>,
    pub producer_version: Option<String>,
    pub opset_versions: Vec<String>,
    // Common
    pub metadata: HashMap<String, String>,
    pub tensors: Vec<TensorInfo>,
    pub tensors_truncated: bool,
}

impl ModelMetadata {
    pub fn new(format: &str, file_size: u64) -> Self {
        Self {
            format: format.to_string(),
            file_size,
            model_name: None,
            architecture: None,
            description: None,
            quantization: None,
            tensor_count: 0,
            total_parameters: 0,
            total_tensor_bytes: 0,
            context_length: None,
            embedding_size: None,
            block_count: None,
            head_count: None,
            head_count_kv: None,
            vocab_size: None,
            gguf_version: None,
            byte_order: None,
            ir_version: None,
            producer_name: None,
            producer_version: None,
            opset_versions: Vec::new(),
            metadata: HashMap::new(),
            tensors: Vec::new(),
            tensors_truncated: false,
        }
    }
}
