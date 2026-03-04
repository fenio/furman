use crate::models::model_metadata::{ModelMetadata, TensorInfo};
use crate::models::FmError;
use std::fs::File;
use std::io::Read;

const MAX_TENSORS: usize = 500;
const MAX_HEADER_READ: u64 = 50_000_000; // Only read first 50MB for header info

/// Minimal protobuf field parser for ONNX ModelProto.
/// ONNX uses protobuf, but we avoid pulling in a protobuf dependency.
/// We only parse top-level fields and the graph's initializer names/shapes.

struct ProtoReader {
    data: Vec<u8>,
    pos: usize,
}

impl ProtoReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_byte(&mut self) -> Result<u8, FmError> {
        if self.pos >= self.data.len() {
            return Err(FmError::Other("ONNX: unexpected end of data".into()));
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_varint(&mut self) -> Result<u64, FmError> {
        let mut result: u64 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.read_byte()?;
            result |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 64 {
                return Err(FmError::Other("ONNX: varint too long".into()));
            }
        }
    }

    fn read_bytes(&mut self, n: usize) -> Result<&[u8], FmError> {
        if self.pos + n > self.data.len() {
            return Err(FmError::Other("ONNX: unexpected end of data".into()));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn read_length_delimited(&mut self) -> Result<Vec<u8>, FmError> {
        let len = self.read_varint()? as usize;
        if len > self.remaining() {
            return Err(FmError::Other("ONNX: length-delimited field exceeds data".into()));
        }
        Ok(self.read_bytes(len)?.to_vec())
    }

    fn read_string(&mut self) -> Result<String, FmError> {
        let bytes = self.read_length_delimited()?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn skip_field(&mut self, wire_type: u32) -> Result<(), FmError> {
        match wire_type {
            0 => { self.read_varint()?; } // varint
            1 => { self.read_bytes(8)?; } // 64-bit
            2 => { let _ = self.read_length_delimited()?; } // length-delimited
            5 => { self.read_bytes(4)?; } // 32-bit
            _ => return Err(FmError::Other(format!("ONNX: unknown wire type {}", wire_type))),
        }
        Ok(())
    }
}

/// Parse top-level fields of an ONNX ModelProto.
pub fn parse(path: &str, file_size: u64) -> Result<ModelMetadata, FmError> {
    let read_size = file_size.min(MAX_HEADER_READ) as usize;

    let mut f = File::open(path)?;
    let mut data = vec![0u8; read_size];
    f.read_exact(&mut data)?;
    // If file is larger, note we only read header portion
    let truncated_read = file_size > MAX_HEADER_READ;
    drop(f);

    let mut r = ProtoReader::new(data);
    let mut meta = ModelMetadata::new("ONNX", file_size);

    // ModelProto fields:
    // 1: ir_version (int64)
    // 2: opset_import (repeated, length-delimited)
    // 3: producer_name (string)
    // 4: producer_version (string)
    // 5: domain (string)
    // 6: model_version (int64)
    // 7: doc_string (string)
    // 8: graph (GraphProto, length-delimited)
    // 14: metadata_props (repeated StringStringEntryProto)

    while r.remaining() > 0 {
        let tag = match r.read_varint() {
            Ok(t) => t,
            Err(_) => break,
        };
        let field_number = (tag >> 3) as u32;
        let wire_type = (tag & 0x7) as u32;

        match (field_number, wire_type) {
            (1, 0) => {
                // ir_version
                meta.ir_version = Some(r.read_varint()?);
            }
            (2, 2) => {
                // opset_import
                let bytes = r.read_length_delimited()?;
                if let Some(opset_str) = parse_opset_import(&bytes) {
                    meta.opset_versions.push(opset_str);
                }
            }
            (3, 2) => {
                meta.producer_name = Some(r.read_string()?);
            }
            (4, 2) => {
                meta.producer_version = Some(r.read_string()?);
            }
            (5, 2) => {
                let domain = r.read_string()?;
                if !domain.is_empty() {
                    meta.metadata.insert("domain".to_string(), domain);
                }
            }
            (6, 0) => {
                let v = r.read_varint()?;
                meta.metadata.insert("model_version".to_string(), v.to_string());
            }
            (7, 2) => {
                let doc = r.read_string()?;
                if !doc.is_empty() {
                    meta.description = Some(doc);
                }
            }
            (8, 2) => {
                // Graph — parse for tensor info
                let graph_bytes = r.read_length_delimited()?;
                parse_graph(&graph_bytes, &mut meta)?;
            }
            (14, 2) => {
                // metadata_props
                let bytes = r.read_length_delimited()?;
                if let Some((k, v)) = parse_string_string_entry(&bytes) {
                    meta.metadata.insert(k, v);
                }
            }
            (_, _) => {
                if let Err(_) = r.skip_field(wire_type) {
                    break;
                }
            }
        }
    }

    if truncated_read && meta.tensors.is_empty() {
        meta.tensors_truncated = true;
    }

    Ok(meta)
}

/// Parse OpsetIdProto: field 1 = domain (string), field 2 = version (int64)
fn parse_opset_import(data: &[u8]) -> Option<String> {
    let mut r = ProtoReader::new(data.to_vec());
    let mut domain = String::new();
    let mut version: u64 = 0;

    while r.remaining() > 0 {
        let tag = r.read_varint().ok()?;
        let field = (tag >> 3) as u32;
        let wt = (tag & 0x7) as u32;
        match (field, wt) {
            (1, 2) => { domain = r.read_string().ok()?; }
            (2, 0) => { version = r.read_varint().ok()?; }
            _ => { r.skip_field(wt).ok()?; }
        }
    }

    if domain.is_empty() {
        Some(format!("ai.onnx v{}", version))
    } else {
        Some(format!("{} v{}", domain, version))
    }
}

/// Parse StringStringEntryProto: field 1 = key, field 2 = value
fn parse_string_string_entry(data: &[u8]) -> Option<(String, String)> {
    let mut r = ProtoReader::new(data.to_vec());
    let mut key = String::new();
    let mut value = String::new();

    while r.remaining() > 0 {
        let tag = r.read_varint().ok()?;
        let field = (tag >> 3) as u32;
        let wt = (tag & 0x7) as u32;
        match (field, wt) {
            (1, 2) => { key = r.read_string().ok()?; }
            (2, 2) => { value = r.read_string().ok()?; }
            _ => { r.skip_field(wt).ok()?; }
        }
    }

    if key.is_empty() { None } else { Some((key, value)) }
}

/// Parse GraphProto for initializer tensors and graph name.
fn parse_graph(data: &[u8], meta: &mut ModelMetadata) -> Result<(), FmError> {
    let mut r = ProtoReader::new(data.to_vec());
    let mut tensor_count: u64 = 0;
    let mut total_params: u64 = 0;

    // GraphProto fields:
    // 1: node (repeated NodeProto)
    // 2: name (string)
    // 5: initializer (repeated TensorProto) — these are the weights
    // 11: input (repeated ValueInfoProto)
    // 12: output (repeated ValueInfoProto)

    while r.remaining() > 0 {
        let tag = match r.read_varint() {
            Ok(t) => t,
            Err(_) => break,
        };
        let field = (tag >> 3) as u32;
        let wt = (tag & 0x7) as u32;

        match (field, wt) {
            (2, 2) => {
                let name = r.read_string()?;
                if !name.is_empty() && meta.model_name.is_none() {
                    meta.model_name = Some(name);
                }
            }
            (5, 2) => {
                // Initializer = TensorProto
                let tensor_bytes = r.read_length_delimited()?;
                if let Some(ti) = parse_tensor_proto(&tensor_bytes) {
                    tensor_count += 1;
                    total_params += ti.param_count;
                    if meta.tensors.len() < MAX_TENSORS {
                        meta.tensors.push(ti);
                    }
                }
            }
            (1, 2) | (11, 2) | (12, 2) => {
                // Skip node, input, output — just consume the bytes
                let _ = r.read_length_delimited()?;
            }
            (_, _) => {
                if let Err(_) = r.skip_field(wt) {
                    break;
                }
            }
        }
    }

    meta.tensor_count = tensor_count;
    meta.total_parameters = total_params;
    meta.total_tensor_bytes = meta.tensors.iter().map(|t| t.size_bytes).sum();
    meta.tensors_truncated = tensor_count as usize > MAX_TENSORS;

    Ok(())
}

/// Parse TensorProto for name, data_type, dims.
/// Fields: 1=dims (repeated int64), 2=data_type (int32), 8=name (string)
/// We skip raw_data (field 13) and other large fields.
fn parse_tensor_proto(data: &[u8]) -> Option<TensorInfo> {
    let mut r = ProtoReader::new(data.to_vec());
    let mut name = String::new();
    let mut data_type: u32 = 0;
    let mut dims: Vec<u64> = Vec::new();

    while r.remaining() > 0 {
        let tag = r.read_varint().ok()?;
        let field = (tag >> 3) as u32;
        let wt = (tag & 0x7) as u32;

        match (field, wt) {
            (1, 0) => {
                // Single dim value
                dims.push(r.read_varint().ok()?);
            }
            (1, 2) => {
                // Packed repeated dims
                let bytes = r.read_length_delimited().ok()?;
                let mut sub = ProtoReader::new(bytes);
                while sub.remaining() > 0 {
                    dims.push(sub.read_varint().ok()?);
                }
            }
            (2, 0) => {
                data_type = r.read_varint().ok()? as u32;
            }
            (8, 2) => {
                name = r.read_string().ok()?;
            }
            (_, _) => {
                r.skip_field(wt).ok()?;
            }
        }
    }

    let dtype_name = onnx_dtype_name(data_type).to_string();
    let param_count: u64 = if dims.is_empty() { 0 } else { dims.iter().product() };
    let elem_size = onnx_dtype_size(data_type);
    let size_bytes = param_count * elem_size;

    Some(TensorInfo {
        name,
        dtype: dtype_name,
        shape: dims,
        size_bytes,
        param_count,
    })
}

fn onnx_dtype_name(dt: u32) -> &'static str {
    match dt {
        1 => "FLOAT",
        2 => "UINT8",
        3 => "INT8",
        4 => "UINT16",
        5 => "INT16",
        6 => "INT32",
        7 => "INT64",
        8 => "STRING",
        9 => "BOOL",
        10 => "FLOAT16",
        11 => "DOUBLE",
        12 => "UINT32",
        13 => "UINT64",
        14 => "COMPLEX64",
        15 => "COMPLEX128",
        16 => "BFLOAT16",
        17 => "FLOAT8E4M3FN",
        18 => "FLOAT8E4M3FNUZ",
        19 => "FLOAT8E5M2",
        20 => "FLOAT8E5M2FNUZ",
        _ => "UNKNOWN",
    }
}

fn onnx_dtype_size(dt: u32) -> u64 {
    match dt {
        1 | 12 => 4,         // FLOAT, UINT32
        2 | 3 | 9 => 1,      // UINT8, INT8, BOOL
        4 | 5 | 10 | 16 => 2, // UINT16, INT16, FLOAT16, BFLOAT16
        6 => 4,               // INT32
        7 | 11 | 13 => 8,    // INT64, DOUBLE, UINT64
        14 => 8,              // COMPLEX64
        15 => 16,             // COMPLEX128
        17..=20 => 1,         // FLOAT8 variants
        _ => 4,               // Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_varint(buf: &mut Vec<u8>, mut val: u64) {
        loop {
            let mut byte = (val & 0x7F) as u8;
            val >>= 7;
            if val > 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if val == 0 {
                break;
            }
        }
    }

    fn write_tag(buf: &mut Vec<u8>, field: u32, wire_type: u32) {
        write_varint(buf, ((field as u64) << 3) | (wire_type as u64));
    }

    fn write_string_field(buf: &mut Vec<u8>, field: u32, s: &str) {
        write_tag(buf, field, 2);
        write_varint(buf, s.len() as u64);
        buf.extend_from_slice(s.as_bytes());
    }

    fn write_varint_field(buf: &mut Vec<u8>, field: u32, val: u64) {
        write_tag(buf, field, 0);
        write_varint(buf, val);
    }

    fn write_length_delimited(buf: &mut Vec<u8>, field: u32, data: &[u8]) {
        write_tag(buf, field, 2);
        write_varint(buf, data.len() as u64);
        buf.extend_from_slice(data);
    }

    #[test]
    fn test_parse_onnx_basic() {
        // Build a minimal ONNX ModelProto
        let mut model = Vec::new();

        // ir_version = 7
        write_varint_field(&mut model, 1, 7);

        // producer_name = "pytorch"
        write_string_field(&mut model, 3, "pytorch");

        // producer_version = "2.0"
        write_string_field(&mut model, 4, "2.0");

        // opset_import: domain="" version=17
        let mut opset = Vec::new();
        write_varint_field(&mut opset, 2, 17);
        write_length_delimited(&mut model, 2, &opset);

        // graph with one initializer tensor
        let mut graph = Vec::new();
        write_string_field(&mut graph, 2, "test_graph");

        // initializer: TensorProto with name="weight", data_type=FLOAT(1), dims=[10,20]
        let mut tensor = Vec::new();
        write_varint_field(&mut tensor, 1, 10); // dim
        write_varint_field(&mut tensor, 1, 20); // dim
        write_varint_field(&mut tensor, 2, 1);  // data_type = FLOAT
        write_string_field(&mut tensor, 8, "weight");
        write_length_delimited(&mut graph, 5, &tensor);

        write_length_delimited(&mut model, 8, &graph);

        // Write to temp file
        let mut f = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        f.write_all(&model).unwrap();
        f.flush().unwrap();

        let path = f.path().to_str().unwrap();
        let meta = parse(path, std::fs::metadata(path).unwrap().len()).unwrap();

        assert_eq!(meta.format, "ONNX");
        assert_eq!(meta.ir_version, Some(7));
        assert_eq!(meta.producer_name.as_deref(), Some("pytorch"));
        assert_eq!(meta.producer_version.as_deref(), Some("2.0"));
        assert_eq!(meta.tensor_count, 1);
        assert_eq!(meta.total_parameters, 200); // 10*20
        assert_eq!(meta.tensors.len(), 1);
        assert_eq!(meta.tensors[0].name, "weight");
        assert_eq!(meta.tensors[0].dtype, "FLOAT");
        assert_eq!(meta.model_name.as_deref(), Some("test_graph"));
        assert_eq!(meta.opset_versions.len(), 1);
    }
}
