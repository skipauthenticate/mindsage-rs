//! int8 quantization/dequantization — matches Python's quantize_uint8/dequantize_uint8.

use ndarray::Array1;

/// Quantize a float32 embedding to uint8 bytes with scale and offset.
///
/// Maps [min, max] → [0, 255] linearly.
/// Returns (bytes, scale, offset) where: original ≈ bytes * scale + offset
pub fn quantize_uint8(embedding: &Array1<f32>) -> (Vec<u8>, f32, f32) {
    let min_val = embedding.iter().copied().fold(f32::INFINITY, f32::min);
    let max_val = embedding.iter().copied().fold(f32::NEG_INFINITY, f32::max);

    let range = max_val - min_val;
    if range < 1e-9 {
        // Constant vector — all zeros
        let bytes = vec![0u8; embedding.len()];
        return (bytes, 0.0, min_val);
    }

    let scale = range / 255.0;
    let offset = min_val;

    let bytes: Vec<u8> = embedding
        .iter()
        .map(|&v| ((v - offset) / scale).round().clamp(0.0, 255.0) as u8)
        .collect();

    (bytes, scale, offset)
}

/// Dequantize uint8 bytes back to float32 embedding.
///
/// original ≈ bytes * scale + offset
pub fn dequantize_uint8(bytes: &[u8], scale: f32, offset: f32) -> Array1<f32> {
    Array1::from_iter(bytes.iter().map(|&b| b as f32 * scale + offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn test_roundtrip() {
        let original = array![0.1, 0.5, -0.3, 0.8, -0.1];
        let (bytes, scale, offset) = quantize_uint8(&original);
        let restored = dequantize_uint8(&bytes, scale, offset);

        for (a, b) in original.iter().zip(restored.iter()) {
            assert!((a - b).abs() < 0.01, "Values differ: {} vs {}", a, b);
        }
    }

    #[test]
    fn test_constant_vector() {
        let original = array![0.5, 0.5, 0.5];
        let (bytes, scale, offset) = quantize_uint8(&original);
        assert_eq!(scale, 0.0);
        assert_eq!(offset, 0.5);
        assert!(bytes.iter().all(|&b| b == 0));
    }
}
