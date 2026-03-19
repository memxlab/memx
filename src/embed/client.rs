use crate::error::{AppError, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct EmbedClient {
    api_key: String,
    base_url: String,
    model: String,
    dimension: usize,
    client: Client,
}

#[derive(Serialize)]
struct EmbedRequest {
    model: String,
    input: String,
    encoding_format: String,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

impl EmbedClient {
    pub fn new(api_key: String, base_url: String, model: String, dimension: usize) -> Self {
        Self {
            api_key,
            base_url: sanitize_base_url(&base_url),
            model,
            dimension,
            client: Client::builder()
                .pool_max_idle_per_host(1)
                .pool_idle_timeout(Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embedding = self.request_embedding(text).await?;
        if embedding.len() != self.dimension {
            return Err(AppError::Embedding(format!(
                "Expected dimension {}, got {}",
                self.dimension,
                embedding.len()
            )));
        }

        Ok(embedding)
    }

    pub async fn detect_dimension(
        api_key: String,
        base_url: String,
        model: String,
        text: &str,
    ) -> Result<usize> {
        let client = Self::new(api_key, base_url, model, 1);
        let embedding = client.request_embedding(text).await?;
        if embedding.is_empty() {
            return Err(AppError::Embedding(
                "Embedding API returned an empty vector".to_string(),
            ));
        }

        Ok(embedding.len())
    }

    async fn request_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.base_url);
        let request = EmbedRequest {
            model: self.model.clone(),
            input: text.to_string(),
            encoding_format: "float".to_string(),
        };

        let mut delay = Duration::from_secs(5);
        for attempt in 0..5 {
            let response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await
                .map_err(|e| AppError::Embedding(format!("Request failed: {}", e)))?;

            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                if attempt < 4 {
                    tracing::debug!(
                        "429 rate limit, retrying in {:?} (attempt {})",
                        delay,
                        attempt + 1
                    );
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                    continue;
                }
                return Err(AppError::Embedding(format!(
                    "API returned 429 after {} retries",
                    attempt + 1
                )));
            }

            if !response.status().is_success() {
                let status = response.status();
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(AppError::Embedding(format!(
                    "API returned {}: {}",
                    status, body
                )));
            }

            let embed_response: EmbedResponse = response
                .json()
                .await
                .map_err(|e| AppError::Embedding(format!("Failed to parse response: {}", e)))?;

            if embed_response.data.is_empty() {
                return Err(AppError::Embedding("Empty response from API".to_string()));
            }

            return Ok(embed_response.data[0].embedding.clone());
        }

        Err(AppError::Embedding("Exhausted retries".to_string()))
    }
}

fn sanitize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}
