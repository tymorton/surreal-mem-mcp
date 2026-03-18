use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
use tokio::sync::Mutex;

pub struct LocalEmbedder {
    model: Mutex<TextEmbedding>,
}

impl LocalEmbedder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::JinaEmbeddingsV2BaseEN)
                .with_show_download_progress(true)
        )?;
        Ok(Self { model: Mutex::new(model) })
    }

    /// Asynchronously compute embeddings. Uses a Tokio Mutex so ONNX inference
    /// (which can take tens of milliseconds) yields the thread back to the
    /// Tokio runtime while waiting for the lock, rather than blocking it.
    pub async fn get_embedding(&self, text: &str) -> Option<Vec<f32>> {
        let mut model = self.model.lock().await;
        model.embed(vec![text], None).ok()?.into_iter().next()
    }
}
