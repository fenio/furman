use crate::models::model_metadata::{ModelMetadata, TensorInfo};
use crate::models::FmError;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const MAX_TENSORS: usize = 500;

// GGML type sizes in bytes (per element) — used for param count estimation
const GGML_TYPE_SIZES: &[(u32, &str, f64)] = &[
    (0, "F32", 4.0),
    (1, "F16", 2.0),
    (2, "Q4_0", 0.5625),   // 4.5 bits
    (3, "Q4_1", 0.625),    // 5 bits
    (6, "Q5_0", 0.6875),   // 5.5 bits
    (7, "Q5_1", 0.75),     // 6 bits
    (8, "Q8_0", 1.0625),   // 8.5 bits
    (9, "Q8_1", 1.125),    // 9 bits
    (10, "Q2_K", 0.3125),
    (11, "Q3_K", 0.4375),
    (12, "Q4_K", 0.5625),
    (13, "Q5_K", 0.6875),
    (14, "Q6_K", 0.8125),
    (15, "Q8_K", 1.0625),
    (16, "IQ2_XXS", 0.25),
    (17, "IQ2_XS", 0.3125),
    (18, "IQ3_XXS", 0.40625),
    (19, "IQ1_S", 0.1875),
    (20, "IQ4_NL", 0.5625),
    (21, "IQ3_S", 0.4375),
    (22, "IQ2_S", 0.3125),
    (23, "IQ4_XS", 0.53125),
    (24, "I8", 1.0),
    (25, "I16", 2.0),
    (26, "I32", 4.0),
    (27, "I64", 8.0),
    (28, "F64", 8.0),
    (29, "IQ1_M", 0.21875),
    (30, "BF16", 2.0),
];

fn ggml_type_name(type_id: u32) -> &'static str {
    for &(id, name, _) in GGML_TYPE_SIZES {
        if id == type_id {
            return name;
        }
    }
    "unknown"
}

fn ggml_type_bytes_per_element(type_id: u32) -> f64 {
    for &(id, _, bpe) in GGML_TYPE_SIZES {
        if id == type_id {
            return bpe;
        }
    }
    1.0
}

/// Map the `general.file_type` integer to a quantization name.
fn file_type_to_quantization(ft: u64) -> Option<&'static str> {
    match ft {
        0 => Some("F32"),
        1 => Some("F16"),
        2 => Some("Q4_0"),
        3 => Some("Q4_1"),
        7 => Some("Q5_0"),
        8 => Some("Q5_1"),
        9 => Some("Q8_0"),
        10 => Some("Q2_K"),
        11 => Some("Q3_K_S"),
        12 => Some("Q3_K_M"),
        13 => Some("Q3_K_L"),
        14 => Some("Q4_K_S"),
        15 => Some("Q4_K_M"),
        16 => Some("Q5_K_S"),
        17 => Some("Q5_K_M"),
        18 => Some("Q6_K"),
        19 => Some("IQ2_XXS"),
        20 => Some("IQ2_XS"),
        21 => Some("IQ3_XXS"),
        22 => Some("IQ1_S"),
        23 => Some("IQ4_NL"),
        24 => Some("IQ3_S"),
        25 => Some("IQ2_S"),
        26 => Some("IQ4_XS"),
        27 => Some("IQ1_M"),
        28 => Some("BF16"),
        _ => None,
    }
}

struct BinaryReader {
    file: File,
}

impl BinaryReader {
    fn new(file: File) -> Self {
        Self { file }
    }

    fn read_u8(&mut self) -> Result<u8, FmError> {
        let mut buf = [0u8; 1];
        self.file.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn read_u16(&mut self) -> Result<u16, FmError> {
        let mut buf = [0u8; 2];
        self.file.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_u32(&mut self) -> Result<u32, FmError> {
        let mut buf = [0u8; 4];
        self.file.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_u64(&mut self) -> Result<u64, FmError> {
        let mut buf = [0u8; 8];
        self.file.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    fn read_i8(&mut self) -> Result<i8, FmError> {
        Ok(self.read_u8()? as i8)
    }

    fn read_i16(&mut self) -> Result<i16, FmError> {
        let mut buf = [0u8; 2];
        self.file.read_exact(&mut buf)?;
        Ok(i16::from_le_bytes(buf))
    }

    fn read_i32(&mut self) -> Result<i32, FmError> {
        let mut buf = [0u8; 4];
        self.file.read_exact(&mut buf)?;
        Ok(i32::from_le_bytes(buf))
    }

    fn read_i64(&mut self) -> Result<i64, FmError> {
        let mut buf = [0u8; 8];
        self.file.read_exact(&mut buf)?;
        Ok(i64::from_le_bytes(buf))
    }

    fn read_f32(&mut self) -> Result<f32, FmError> {
        let mut buf = [0u8; 4];
        self.file.read_exact(&mut buf)?;
        Ok(f32::from_le_bytes(buf))
    }

    fn read_f64(&mut self) -> Result<f64, FmError> {
        let mut buf = [0u8; 8];
        self.file.read_exact(&mut buf)?;
        Ok(f64::from_le_bytes(buf))
    }

    fn read_string(&mut self) -> Result<String, FmError> {
        let len = self.read_u64()? as usize;
        if len > 10_000_000 {
            return Err(FmError::Other(format!("GGUF string too long: {}", len)));
        }
        let mut buf = vec![0u8; len];
        self.file.read_exact(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn read_bool(&mut self) -> Result<bool, FmError> {
        Ok(self.read_u8()? != 0)
    }

    fn skip(&mut self, n: u64) -> Result<(), FmError> {
        self.file.seek(SeekFrom::Current(n as i64))?;
        Ok(())
    }
}

// GGUF value types
const GGUF_TYPE_UINT8: u32 = 0;
const GGUF_TYPE_INT8: u32 = 1;
const GGUF_TYPE_UINT16: u32 = 2;
const GGUF_TYPE_INT16: u32 = 3;
const GGUF_TYPE_UINT32: u32 = 4;
const GGUF_TYPE_INT32: u32 = 5;
const GGUF_TYPE_FLOAT32: u32 = 6;
const GGUF_TYPE_BOOL: u32 = 7;
const GGUF_TYPE_STRING: u32 = 8;
const GGUF_TYPE_ARRAY: u32 = 9;
const GGUF_TYPE_UINT64: u32 = 10;
const GGUF_TYPE_INT64: u32 = 11;
const GGUF_TYPE_FLOAT64: u32 = 12;

#[derive(Debug, Clone)]
enum GgufValue {
    Uint8(u8),
    Int8(i8),
    Uint16(u16),
    Int16(i16),
    Uint32(u32),
    Int32(i32),
    Float32(f32),
    Bool(bool),
    String(String),
    Uint64(u64),
    Int64(i64),
    Float64(f64),
    Array(Vec<GgufValue>),
}

impl GgufValue {
    fn as_u64(&self) -> Option<u64> {
        match self {
            GgufValue::Uint8(v) => Some(*v as u64),
            GgufValue::Uint16(v) => Some(*v as u64),
            GgufValue::Uint32(v) => Some(*v as u64),
            GgufValue::Uint64(v) => Some(*v),
            GgufValue::Int8(v) => Some(*v as u64),
            GgufValue::Int16(v) => Some(*v as u64),
            GgufValue::Int32(v) => Some(*v as u64),
            GgufValue::Int64(v) => Some(*v as u64),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            GgufValue::String(s) => Some(s),
            _ => None,
        }
    }

    fn to_string_lossy(&self) -> String {
        match self {
            GgufValue::String(s) => s.clone(),
            GgufValue::Uint8(v) => v.to_string(),
            GgufValue::Int8(v) => v.to_string(),
            GgufValue::Uint16(v) => v.to_string(),
            GgufValue::Int16(v) => v.to_string(),
            GgufValue::Uint32(v) => v.to_string(),
            GgufValue::Int32(v) => v.to_string(),
            GgufValue::Uint64(v) => v.to_string(),
            GgufValue::Int64(v) => v.to_string(),
            GgufValue::Float32(v) => v.to_string(),
            GgufValue::Float64(v) => v.to_string(),
            GgufValue::Bool(v) => v.to_string(),
            GgufValue::Array(_) => "[array]".to_string(),
        }
    }
}

fn read_value(r: &mut BinaryReader, value_type: u32) -> Result<GgufValue, FmError> {
    match value_type {
        GGUF_TYPE_UINT8 => Ok(GgufValue::Uint8(r.read_u8()?)),
        GGUF_TYPE_INT8 => Ok(GgufValue::Int8(r.read_i8()?)),
        GGUF_TYPE_UINT16 => Ok(GgufValue::Uint16(r.read_u16()?)),
        GGUF_TYPE_INT16 => Ok(GgufValue::Int16(r.read_i16()?)),
        GGUF_TYPE_UINT32 => Ok(GgufValue::Uint32(r.read_u32()?)),
        GGUF_TYPE_INT32 => Ok(GgufValue::Int32(r.read_i32()?)),
        GGUF_TYPE_FLOAT32 => Ok(GgufValue::Float32(r.read_f32()?)),
        GGUF_TYPE_BOOL => Ok(GgufValue::Bool(r.read_bool()?)),
        GGUF_TYPE_STRING => Ok(GgufValue::String(r.read_string()?)),
        GGUF_TYPE_UINT64 => Ok(GgufValue::Uint64(r.read_u64()?)),
        GGUF_TYPE_INT64 => Ok(GgufValue::Int64(r.read_i64()?)),
        GGUF_TYPE_FLOAT64 => Ok(GgufValue::Float64(r.read_f64()?)),
        GGUF_TYPE_ARRAY => {
            let elem_type = r.read_u32()?;
            let count = r.read_u64()? as usize;
            // Limit array size to prevent OOM
            if count > 100_000 {
                // Skip large arrays
                for _ in 0..count {
                    skip_value(r, elem_type)?;
                }
                return Ok(GgufValue::Array(Vec::new()));
            }
            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(read_value(r, elem_type)?);
            }
            Ok(GgufValue::Array(items))
        }
        _ => Err(FmError::Other(format!("Unknown GGUF value type: {}", value_type))),
    }
}

fn skip_value(r: &mut BinaryReader, value_type: u32) -> Result<(), FmError> {
    match value_type {
        GGUF_TYPE_UINT8 | GGUF_TYPE_INT8 | GGUF_TYPE_BOOL => { r.skip(1)?; }
        GGUF_TYPE_UINT16 | GGUF_TYPE_INT16 => { r.skip(2)?; }
        GGUF_TYPE_UINT32 | GGUF_TYPE_INT32 | GGUF_TYPE_FLOAT32 => { r.skip(4)?; }
        GGUF_TYPE_UINT64 | GGUF_TYPE_INT64 | GGUF_TYPE_FLOAT64 => { r.skip(8)?; }
        GGUF_TYPE_STRING => { let _ = r.read_string()?; }
        GGUF_TYPE_ARRAY => {
            let elem_type = r.read_u32()?;
            let count = r.read_u64()?;
            for _ in 0..count {
                skip_value(r, elem_type)?;
            }
        }
        _ => return Err(FmError::Other(format!("Unknown GGUF value type: {}", value_type))),
    }
    Ok(())
}

/// Parse a GGUF file's header: magic, version, KV pairs, tensor info entries.
pub fn parse(path: &str, file_size: u64) -> Result<ModelMetadata, FmError> {
    let f = File::open(path)?;
    let mut r = BinaryReader::new(f);

    // Magic (already validated)
    r.skip(4)?;

    let version = r.read_u32()?;
    if version < 2 || version > 3 {
        return Err(FmError::Other(format!("Unsupported GGUF version: {}", version)));
    }

    let tensor_count = r.read_u64()?;
    let kv_count = r.read_u64()?;

    let mut meta = ModelMetadata::new("GGUF", file_size);
    meta.gguf_version = Some(version);
    meta.tensor_count = tensor_count;

    // Parse KV pairs
    let mut kv_map: std::collections::HashMap<String, GgufValue> = std::collections::HashMap::new();
    for _ in 0..kv_count {
        let key = r.read_string()?;
        let value_type = r.read_u32()?;
        let value = read_value(&mut r, value_type)?;
        kv_map.insert(key, value);
    }

    // Extract well-known keys
    let arch = kv_map
        .get("general.architecture")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if !arch.is_empty() {
        meta.architecture = Some(arch.clone());
    }

    if let Some(v) = kv_map.get("general.name").and_then(|v| v.as_str()) {
        meta.model_name = Some(v.to_string());
    }

    if let Some(v) = kv_map.get("general.description").and_then(|v| v.as_str()) {
        meta.description = Some(v.to_string());
    }

    if let Some(v) = kv_map.get("general.file_type").and_then(|v| v.as_u64()) {
        if let Some(q) = file_type_to_quantization(v) {
            meta.quantization = Some(q.to_string());
        }
    }

    // Architecture-specific keys
    let arch_prefix = if arch.is_empty() { String::new() } else { format!("{}.", arch) };

    if let Some(v) = kv_map.get(&format!("{}context_length", arch_prefix)).and_then(|v| v.as_u64()) {
        meta.context_length = Some(v);
    }
    if let Some(v) = kv_map.get(&format!("{}embedding_length", arch_prefix)).and_then(|v| v.as_u64()) {
        meta.embedding_size = Some(v);
    }
    if let Some(v) = kv_map.get(&format!("{}block_count", arch_prefix)).and_then(|v| v.as_u64()) {
        meta.block_count = Some(v);
    }
    if let Some(v) = kv_map.get(&format!("{}attention.head_count", arch_prefix)).and_then(|v| v.as_u64()) {
        meta.head_count = Some(v);
    }
    if let Some(v) = kv_map.get(&format!("{}attention.head_count_kv", arch_prefix)).and_then(|v| v.as_u64()) {
        meta.head_count_kv = Some(v);
    }

    // Vocab size: try tokenizer key
    if let Some(v) = kv_map.get("tokenizer.ggml.tokens").and_then(|v| {
        if let GgufValue::Array(arr) = v { Some(arr.len() as u64) } else { None }
    }) {
        meta.vocab_size = Some(v);
    }

    // Store all metadata as strings
    for (key, value) in &kv_map {
        // Skip very large arrays (like token lists)
        if let GgufValue::Array(arr) = value {
            if arr.len() > 100 {
                meta.metadata.insert(key.clone(), format!("[{} items]", arr.len()));
                continue;
            }
        }
        meta.metadata.insert(key.clone(), value.to_string_lossy());
    }

    // Parse tensor info entries
    let mut total_params: u64 = 0;
    let mut tensors: Vec<TensorInfo> = Vec::new();

    for _ in 0..tensor_count {
        let name = r.read_string()?;
        let ndims = r.read_u32()?;
        let mut shape = Vec::with_capacity(ndims as usize);
        for _ in 0..ndims {
            shape.push(r.read_u64()?);
        }
        let ggml_type = r.read_u32()?;
        let _offset = r.read_u64()?;

        let param_count: u64 = if shape.is_empty() { 0 } else { shape.iter().product() };
        total_params += param_count;

        let bytes_per_elem = ggml_type_bytes_per_element(ggml_type);
        let size_bytes = (param_count as f64 * bytes_per_elem) as u64;

        if tensors.len() < MAX_TENSORS {
            tensors.push(TensorInfo {
                name,
                dtype: ggml_type_name(ggml_type).to_string(),
                shape,
                size_bytes,
                param_count,
            });
        }
    }

    meta.total_parameters = total_params;
    meta.total_tensor_bytes = tensors.iter().map(|t| t.size_bytes).sum();
    meta.tensors_truncated = tensor_count as usize > MAX_TENSORS;
    meta.tensors = tensors;

    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_string(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    }

    fn make_minimal_gguf() -> NamedTempFile {
        let mut buf: Vec<u8> = Vec::new();

        // Magic
        buf.extend_from_slice(b"GGUF");
        // Version 3
        buf.extend_from_slice(&3u32.to_le_bytes());
        // Tensor count: 1
        buf.extend_from_slice(&1u64.to_le_bytes());
        // KV count: 2
        buf.extend_from_slice(&2u64.to_le_bytes());

        // KV 1: general.architecture = "llama"
        write_string(&mut buf, "general.architecture");
        buf.extend_from_slice(&(GGUF_TYPE_STRING as u32).to_le_bytes());
        write_string(&mut buf, "llama");

        // KV 2: general.name = "TestModel"
        write_string(&mut buf, "general.name");
        buf.extend_from_slice(&(GGUF_TYPE_STRING as u32).to_le_bytes());
        write_string(&mut buf, "TestModel");

        // Tensor info: "output.weight", 2 dims [32, 64], type F16, offset 0
        write_string(&mut buf, "output.weight");
        buf.extend_from_slice(&2u32.to_le_bytes()); // ndims
        buf.extend_from_slice(&32u64.to_le_bytes()); // dim 0
        buf.extend_from_slice(&64u64.to_le_bytes()); // dim 1
        buf.extend_from_slice(&1u32.to_le_bytes()); // type = F16
        buf.extend_from_slice(&0u64.to_le_bytes()); // offset

        let mut f = NamedTempFile::new().unwrap();
        f.write_all(&buf).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn test_parse_gguf() {
        let f = make_minimal_gguf();
        let path = f.path().to_str().unwrap();
        let meta = parse(path, std::fs::metadata(path).unwrap().len()).unwrap();

        assert_eq!(meta.format, "GGUF");
        assert_eq!(meta.gguf_version, Some(3));
        assert_eq!(meta.architecture.as_deref(), Some("llama"));
        assert_eq!(meta.model_name.as_deref(), Some("TestModel"));
        assert_eq!(meta.tensor_count, 1);
        assert_eq!(meta.total_parameters, 32 * 64);
        assert_eq!(meta.tensors.len(), 1);
        assert_eq!(meta.tensors[0].dtype, "F16");
    }

    #[test]
    fn test_file_type_to_quantization() {
        assert_eq!(file_type_to_quantization(0), Some("F32"));
        assert_eq!(file_type_to_quantization(15), Some("Q4_K_M"));
        assert_eq!(file_type_to_quantization(999), None);
    }
}
