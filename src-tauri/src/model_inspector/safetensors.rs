use crate::models::model_metadata::{ModelMetadata, TensorInfo};
use crate::models::FmError;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const MAX_HEADER_SIZE: u64 = 100_000_000; // 100MB
const MAX_TENSORS: usize = 500;

/// Parse a SafeTensors file by reading just the JSON header.
pub fn parse(path: &str, file_size: u64) -> Result<ModelMetadata, FmError> {
    let mut f = File::open(path)?;

    // Read 8-byte LE u64 header length
    let mut len_buf = [0u8; 8];
    f.read_exact(&mut len_buf)?;
    let header_len = u64::from_le_bytes(len_buf);

    if header_len == 0 || header_len > MAX_HEADER_SIZE {
        return Err(FmError::Other(format!(
            "Invalid SafeTensors header length: {}",
            header_len
        )));
    }

    // Validate header doesn't exceed file size
    if header_len + 8 > file_size {
        return Err(FmError::Other(
            "SafeTensors header exceeds file size".into(),
        ));
    }

    // Read JSON header
    f.seek(SeekFrom::Start(8))?;
    let mut header_buf = vec![0u8; header_len as usize];
    f.read_exact(&mut header_buf)?;

    let header: serde_json::Value = serde_json::from_slice(&header_buf)
        .map_err(|e| FmError::Other(format!("Invalid SafeTensors JSON header: {}", e)))?;

    let obj = header
        .as_object()
        .ok_or_else(|| FmError::Other("SafeTensors header is not a JSON object".into()))?;

    let mut meta = ModelMetadata::new("SafeTensors", file_size);
    let mut total_params: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut tensors: Vec<TensorInfo> = Vec::new();
    let mut tensor_count: u64 = 0;

    for (key, value) in obj {
        if key == "__metadata__" {
            // Extract metadata key-value pairs
            if let Some(meta_obj) = value.as_object() {
                for (mk, mv) in meta_obj {
                    if let Some(s) = mv.as_str() {
                        meta.metadata.insert(mk.clone(), s.to_string());
                    }
                }
            }
            continue;
        }

        // Each key is a tensor name
        tensor_count += 1;
        let tensor_obj = match value.as_object() {
            Some(o) => o,
            None => continue,
        };

        let dtype = tensor_obj
            .get("dtype")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let shape: Vec<u64> = tensor_obj
            .get("shape")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_u64())
                    .collect()
            })
            .unwrap_or_default();

        let param_count: u64 = if shape.is_empty() {
            0
        } else {
            shape.iter().product()
        };
        total_params += param_count;

        // Compute size from data_offsets
        let size_bytes = tensor_obj
            .get("data_offsets")
            .and_then(|v| v.as_array())
            .and_then(|arr| {
                if arr.len() == 2 {
                    let start = arr[0].as_u64()?;
                    let end = arr[1].as_u64()?;
                    Some(end - start)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| param_count * dtype_size(&dtype));
        total_bytes += size_bytes;

        if tensors.len() < MAX_TENSORS {
            tensors.push(TensorInfo {
                name: key.clone(),
                dtype,
                shape,
                size_bytes,
                param_count,
            });
        }
    }

    meta.tensor_count = tensor_count;
    meta.total_parameters = total_params;
    meta.total_tensor_bytes = total_bytes;
    meta.tensors_truncated = tensor_count as usize > MAX_TENSORS;
    meta.tensors = tensors;

    // Try to extract model name from metadata
    if let Some(name) = meta.metadata.get("model_name").or_else(|| meta.metadata.get("general.name")) {
        meta.model_name = Some(name.clone());
    }
    if let Some(arch) = meta.metadata.get("architecture").or_else(|| meta.metadata.get("general.architecture")) {
        meta.architecture = Some(arch.clone());
    }
    if let Some(desc) = meta.metadata.get("description").or_else(|| meta.metadata.get("general.description")) {
        meta.description = Some(desc.clone());
    }

    Ok(meta)
}

fn dtype_size(dtype: &str) -> u64 {
    match dtype {
        "F64" | "I64" | "U64" => 8,
        "F32" | "I32" | "U32" => 4,
        "F16" | "BF16" | "I16" | "U16" => 2,
        "I8" | "U8" | "BOOL" => 1,
        _ => 2, // Default to 2 bytes for unknown types
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_safetensors(header_json: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        let header_bytes = header_json.as_bytes();
        let len = header_bytes.len() as u64;
        f.write_all(&len.to_le_bytes()).unwrap();
        f.write_all(header_bytes).unwrap();
        // Write some dummy tensor data
        f.write_all(&[0u8; 64]).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn test_parse_basic_safetensors() {
        let header = r#"{
            "__metadata__": {"model_name": "test-model"},
            "weight": {"dtype": "F32", "shape": [10, 20], "data_offsets": [0, 800]},
            "bias": {"dtype": "F32", "shape": [20], "data_offsets": [800, 880]}
        }"#;
        let f = make_safetensors(header);
        let path = f.path().to_str().unwrap();
        let meta = parse(path, std::fs::metadata(path).unwrap().len()).unwrap();

        assert_eq!(meta.format, "SafeTensors");
        assert_eq!(meta.tensor_count, 2);
        assert_eq!(meta.total_parameters, 220); // 10*20 + 20
        assert_eq!(meta.model_name.as_deref(), Some("test-model"));
        assert_eq!(meta.tensors.len(), 2);
        assert!(!meta.tensors_truncated);
    }

    #[test]
    fn test_parse_empty_metadata() {
        let header = r#"{
            "layer.0.weight": {"dtype": "F16", "shape": [512, 768], "data_offsets": [0, 786432]}
        }"#;
        let f = make_safetensors(header);
        let path = f.path().to_str().unwrap();
        let meta = parse(path, std::fs::metadata(path).unwrap().len()).unwrap();

        assert_eq!(meta.tensor_count, 1);
        assert_eq!(meta.total_parameters, 512 * 768);
        assert!(meta.model_name.is_none());
    }

    #[test]
    fn test_invalid_header_length() {
        let mut f = NamedTempFile::new().unwrap();
        let len: u64 = 999_999_999; // Too large
        f.write_all(&len.to_le_bytes()).unwrap();
        f.flush().unwrap();
        let path = f.path().to_str().unwrap();
        let result = parse(path, std::fs::metadata(path).unwrap().len());
        assert!(result.is_err());
    }
}
