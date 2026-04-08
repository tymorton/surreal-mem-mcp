use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
use std::env;
use tokio::sync::Mutex;

pub struct LocalEmbedder {
    model: Mutex<TextEmbedding>,
    dimensions: usize,
}

impl LocalEmbedder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let model_str = env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "JinaEmbeddingsV2BaseEN".to_string());
        
        let (model_enum, dimensions) = match model_str.as_str() {
            "AllMiniLML6V2" => (EmbeddingModel::AllMiniLML6V2, 384),
            "BGESmallENV15" => (EmbeddingModel::BGESmallENV15, 384),
            "NomicEmbedTextV15" => (EmbeddingModel::NomicEmbedTextV15, 768),
            "BGEBaseENV15" => (EmbeddingModel::BGEBaseENV15, 768),
            _ => (EmbeddingModel::JinaEmbeddingsV2BaseEN, 768), // Default
        };

        println!("Initializing Embedding Engine: {} ({} dimensions)", model_str, dimensions);

        let model = TextEmbedding::try_new(
            InitOptions::new(model_enum)
                .with_show_download_progress(true)
        )?;
        Ok(Self { model: Mutex::new(model), dimensions })
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Asynchronously compute embeddings. Uses a Tokio Mutex so ONNX inference
    /// (which can take tens of milliseconds) yields the thread back to the
    /// Tokio runtime while waiting for the lock, rather than blocking it.
    pub async fn get_embedding(&self, text: &str) -> Option<Vec<f32>> {
        let mut model = self.model.lock().await;
        model.embed(vec![text], None).ok()?.into_iter().next()
    }
}
