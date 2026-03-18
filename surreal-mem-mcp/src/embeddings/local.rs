use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};
use std::sync::RwLock;

pub struct LocalEmbedder {
    model: RwLock<TextEmbedding>,
}

impl LocalEmbedder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::JinaEmbeddingsV2BaseEN)
                .with_show_download_progress(true)
        )?;
        Ok(Self { model: RwLock::new(model) })
    }

    pub fn get_embedding(&self, text: &str) -> Option<Vec<f32>> {
        if let Ok(mut model) = self.model.write() {
            if let Ok(embeddings) = model.embed(vec![text], None) {
                 return embeddings.into_iter().next();
            }
        }
        None
    }
}
