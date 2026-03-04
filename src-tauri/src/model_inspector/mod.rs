mod gguf;
mod onnx;
mod safetensors;

use crate::models::model_metadata::ModelMetadata;
use crate::models::FmError;
use std::fs::File;
use std::io::Read;

/// Inspect a model file, reading only header data for fast metadata extraction.
pub fn inspect(path: &str) -> Result<ModelMetadata, FmError> {
    let file_size = std::fs::metadata(path)?.len();

    // Read first 8 bytes for magic detection
    let mut f = File::open(path)?;
    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)?;
    drop(f);

    // GGUF magic: bytes "GGUF"
    if &magic[0..4] == b"GGUF" {
        return gguf::parse(path, file_size);
    }

    // Check extension for format hints
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "safetensors" => safetensors::parse(path, file_size),
        "onnx" => onnx::parse(path, file_size),
        _ => {
            // Heuristic: try safetensors (first 8 bytes as LE u64 header length < 100MB)
            let header_len = u64::from_le_bytes(magic);
            if header_len > 0 && header_len < 100_000_000 {
                safetensors::parse(path, file_size)
            } else {
                Err(FmError::Other(format!(
                    "Unsupported model format: {}",
                    ext
                )))
            }
        }
    }
}
