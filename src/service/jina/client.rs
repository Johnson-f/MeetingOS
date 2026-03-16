use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config::JinaConfig;

#[derive(Clone)]
pub struct JinaClient {
    http: Client,
    api_key: String,
    base_url: String,
    pub embedding_model: String,
    pub reranker_model: String,
}

#[derive(Debug, Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
    task: String,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDataItem>,
}

#[derive(Debug, Deserialize)]
struct EmbedDataItem {
    embedding: Vec<f32>,
}

#[derive(Debug, Serialize)]
struct RerankRequest {
    model: String,
    query: String,
    documents: Vec<String>,
    top_n: usize,
    return_documents: bool,
}

#[derive(Debug, Deserialize)]
pub struct RerankResponse {
    pub results: Vec<RerankResult>,
}

#[derive(Debug, Deserialize)]
pub struct RerankResult {
    pub index: usize,
    pub relevance_score: f64,
}

impl JinaClient {
    pub fn new(config: &JinaConfig) -> Option<Self> {
        let api_key = config.api_key.as_ref()?;
        info!("Jina client initialized");
        Some(Self {
            http: Client::new(),
            api_key: api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_owned(),
            embedding_model: config.embedding_model.clone(),
            reranker_model: config.reranker_model.clone(),
        })
    }

    /// Embed texts for storage (passages) or queries
    /// task: "retrieval.passage" for indexing, "retrieval.query" for search queries
    pub async fn embed(&self, texts: Vec<String>, task: &str) -> Result<Vec<Vec<f32>>> {
        let count = texts.len();
        let response = self
            .http
            .post(format!("{}/v1/embeddings", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&EmbedRequest {
                model: self.embedding_model.clone(),
                input: texts,
                task: task.to_owned(),
            })
            .send()
            .await?
            .error_for_status()
            .map_err(|e| {
                warn!(error = %e, "Jina embed request failed");
                e
            })?
            .json::<EmbedResponse>()
            .await
            .context("failed to parse Jina embed response")?;

        info!(
            count = count,
            dims = response
                .data
                .first()
                .map(|d| d.embedding.len())
                .unwrap_or(0),
            "embedded texts via Jina"
        );
        Ok(response.data.into_iter().map(|d| d.embedding).collect())
    }

    /// Rerank documents against a query
    pub async fn rerank(
        &self,
        query: &str,
        documents: Vec<String>,
        top_n: usize,
    ) -> Result<RerankResponse> {
        let doc_count = documents.len();
        let response = self
            .http
            .post(format!("{}/v1/rerank", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&RerankRequest {
                model: self.reranker_model.clone(),
                query: query.to_owned(),
                documents,
                top_n,
                return_documents: false,
            })
            .send()
            .await?
            .error_for_status()
            .map_err(|e| {
                warn!(error = %e, "Jina rerank request failed");
                e
            })?
            .json::<RerankResponse>()
            .await
            .context("failed to parse Jina rerank response")?;

        info!(
            input_docs = doc_count,
            output_docs = response.results.len(),
            "reranked via Jina"
        );
        Ok(response)
    }
}
