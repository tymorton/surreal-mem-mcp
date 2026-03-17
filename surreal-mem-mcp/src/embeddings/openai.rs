use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct OpenAiEmbeddingRequest {
    model: String,
    input: String,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingData>,
}

pub struct OpenAiClient {
    client: Client,
    endpoint: String,
    model: String,
    api_key: String,
}

impl OpenAiClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            endpoint: std::env::var("EMBEDDING_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1/embeddings".to_string()),
            model: std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-small".to_string()),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "".to_string()),
        }
    }

    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let req_body = OpenAiEmbeddingRequest {
            model: self.model.clone(),
            input: text.to_string(),
        };

        let mut req = self.client.post(&self.endpoint);

        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }

        let res = req.json(&req_body).send().await?;

        let res_data: OpenAiEmbeddingResponse = res.json().await?;

        if let Some(first) = res_data.data.into_iter().next() {
            Ok(first.embedding)
        } else {
            Err("No embedding returned".into())
        }
    }
}
