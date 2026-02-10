//! ONNX-based embedding engine using all-MiniLM-L6-v2.
//!
//! Loads a SentenceTransformers ONNX model and tokenizer to generate
//! 384-dimensional float32 embeddings. Requires the `onnx` feature.

#[cfg(feature = "onnx")]
mod inner {
    use std::path::Path;
    use std::sync::Arc;

    use ndarray::Array1;
    use ort::session::Session;
    use ort::value::Tensor;
    use parking_lot::Mutex;
    use tokenizers::Tokenizer;
    use tracing::{info, warn};

    use crate::cache::QueryCache;
    use crate::embedder::{EmbedderBackend, EmbeddingResult};

    /// Maximum sequence length for the model.
    const MAX_SEQ_LEN: usize = 512;

    /// Default embedding dimension (all-MiniLM-L6-v2).
    const DEFAULT_DIM: usize = 384;

    /// ONNX embedding engine using all-MiniLM-L6-v2.
    pub struct OnnxEmbedder {
        session: Arc<Mutex<Session>>,
        tokenizer: Tokenizer,
        cache: QueryCache,
        dimension: usize,
    }

    impl OnnxEmbedder {
        /// Load an ONNX model and tokenizer from the given directory.
        ///
        /// Expects:
        /// - `model_dir/model.onnx` — the ONNX model file
        /// - `model_dir/tokenizer.json` — the HuggingFace tokenizer
        pub fn load(model_dir: &Path) -> Result<Self, String> {
            let model_path = model_dir.join("model.onnx");
            let tokenizer_path = model_dir.join("tokenizer.json");

            if !model_path.exists() {
                return Err(format!("Model not found: {}", model_path.display()));
            }
            if !tokenizer_path.exists() {
                return Err(format!("Tokenizer not found: {}", tokenizer_path.display()));
            }

            // Initialize ONNX Runtime environment.
            // With load-dynamic feature, ORT_DYLIB_PATH env var must point to libonnxruntime.so
            ort::init().commit();

            let session = Session::builder()
                .map_err(|e| format!("Failed to create session builder: {}", e))?
                .with_intra_threads(2)
                .map_err(|e| format!("Failed to set threads: {}", e))?
                .commit_from_file(&model_path)
                .map_err(|e| format!("Failed to load ONNX model: {}", e))?;

            let tokenizer = Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| format!("Failed to load tokenizer: {}", e))?;

            info!(
                "ONNX embedder loaded: dim={}, model={}",
                DEFAULT_DIM,
                model_path.display()
            );

            Ok(Self {
                session: Arc::new(Mutex::new(session)),
                tokenizer,
                cache: QueryCache::default_cache(),
                dimension: DEFAULT_DIM,
            })
        }

        /// Run inference on tokenized input.
        fn infer(&self, text: &str) -> Option<Array1<f32>> {
            // Tokenize
            let encoding = self
                .tokenizer
                .encode(text, true)
                .map_err(|e| {
                    warn!("Tokenization failed: {}", e);
                    e
                })
                .ok()?;

            let input_ids = encoding.get_ids();
            let attention_mask = encoding.get_attention_mask();

            // Truncate to max sequence length
            let seq_len = input_ids.len().min(MAX_SEQ_LEN);
            let input_ids = &input_ids[..seq_len];
            let attention_mask = &attention_mask[..seq_len];

            // Build input tensors via ort::Tensor::from_array with (shape, data) tuples
            let ids_data: Vec<i64> = input_ids.iter().map(|&id| id as i64).collect();
            let mask_data: Vec<i64> = attention_mask.iter().map(|&m| m as i64).collect();
            let type_ids_data: Vec<i64> = vec![0i64; seq_len];

            let ids_tensor = Tensor::from_array(([1usize, seq_len], ids_data))
                .map_err(|e| warn!("Failed to create ids tensor: {}", e))
                .ok()?;
            let mask_tensor = Tensor::from_array(([1usize, seq_len], mask_data))
                .map_err(|e| warn!("Failed to create mask tensor: {}", e))
                .ok()?;
            let type_ids_tensor = Tensor::from_array(([1usize, seq_len], type_ids_data))
                .map_err(|e| warn!("Failed to create type_ids tensor: {}", e))
                .ok()?;

            let mut session = self.session.lock();
            let outputs = session
                .run(ort::inputs![ids_tensor, mask_tensor, type_ids_tensor])
                .map_err(|e| {
                    warn!("ONNX inference failed: {}", e);
                    e
                })
                .ok()?;

            // Get first output tensor
            // SentenceTransformers models output either:
            //   [1, seq_len, dim] (token_embeddings) → needs mean pooling
            //   [1, dim] (sentence_embedding) → already pooled
            let (shape, data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| {
                    warn!("Failed to extract output tensor: {}", e);
                    e
                })
                .ok()?;

            let shape_dims: Vec<i64> = shape.iter().copied().collect();

            let embedding = if shape_dims.len() == 3 {
                // Token embeddings [1, seq_len, dim] → mean pooling with attention mask
                let dim = shape_dims[2] as usize;
                let mask_f32: Vec<f32> = attention_mask.iter().map(|&m| m as f32).collect();
                let mask_sum: f32 = mask_f32.iter().sum();
                if mask_sum < 1e-9 {
                    return None;
                }

                // data is laid out as [batch=1][seq_len][dim]
                let mut pooled = Array1::zeros(dim);
                for (i, &m) in mask_f32.iter().enumerate() {
                    if m > 0.0 {
                        let offset = i * dim;
                        for d in 0..dim {
                            pooled[d] += data[offset + d] * m;
                        }
                    }
                }
                pooled / mask_sum
            } else if shape_dims.len() == 2 {
                // Already pooled [1, dim]
                let dim = shape_dims[1] as usize;
                Array1::from_vec(data[..dim].to_vec())
            } else {
                warn!("Unexpected output shape: {:?}", shape_dims);
                return None;
            };

            Some(embedding)
        }
    }

    impl EmbedderBackend for OnnxEmbedder {
        fn embed(&self, text: &str) -> Option<EmbeddingResult> {
            // Check cache first
            if let Some(cached) = self.cache.get(text) {
                return Some(EmbeddingResult {
                    embedding: cached,
                    cached: true,
                });
            }

            let embedding = self.infer(text)?;
            self.cache.put(text.to_string(), embedding.clone());

            Some(EmbeddingResult {
                embedding,
                cached: false,
            })
        }

        fn embed_batch(&self, texts: &[&str]) -> Vec<Option<EmbeddingResult>> {
            // Sequential for now; batch inference can be added later
            texts.iter().map(|t| self.embed(t)).collect()
        }

        fn dimension(&self) -> usize {
            self.dimension
        }

        fn is_available(&self) -> bool {
            true
        }
    }
}

#[cfg(feature = "onnx")]
pub use inner::OnnxEmbedder;
