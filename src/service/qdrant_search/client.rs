use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, CreateFieldIndexCollectionBuilder,
    DeletePointsBuilder, Distance, FieldType, Filter, Fusion,
    PrefetchQueryBuilder, PointStruct, Query, QueryPointsBuilder,
    SparseVectorConfig, SparseVectorParamsBuilder, SparseVectorsConfigBuilder,
    UpsertPointsBuilder, Value as QdrantValue, VectorParamsBuilder,
    VectorsConfig, VectorsConfigBuilder,
};
use qdrant_client::Qdrant;
use tracing::{info, warn};


use crate::config::QdrantConfig;

const DENSE_VECTOR_SIZE: u64 = 1024; // jina-embeddings-v3
const DENSE_VECTOR_NAME: &str = "dense";
const SPARSE_VECTOR_NAME: &str = "sparse";

#[derive(Clone)]
pub struct QdrantClient {
    client: Qdrant,
    pub collection_name: String,
}

#[derive(Debug, Clone)]
pub struct ChunkPoint {
    pub id: String,
    pub meeting_id: String,
    pub meeting_title: String,
    pub user_id: String,
    pub chunk_index: usize,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_label: Option<String>,
    pub dense_vector: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub meeting_id: String,
    pub meeting_title: String,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_label: Option<String>,
}

impl QdrantClient {
    pub async fn connect(config: &QdrantConfig) -> Option<Self> {
        let url = config.url.as_deref()?;

        let mut builder = Qdrant::from_url(url)
            .skip_compatibility_check();
        if let Some(api_key) = &config.api_key {
            builder = builder.api_key(api_key.as_str());
        }

        match builder.build() {
            Ok(client) => {
                info!("Qdrant client connected to {}", url);
                let qdrant = Self {
                    client,
                    collection_name: config.collection_name.clone(),
                };
                if let Err(e) = qdrant.ensure_collection().await {
                    warn!(error = %e, "failed to ensure Qdrant collection");
                    return None;
                }
                Some(qdrant)
            }
            Err(e) => {
                warn!(error = %e, "failed to create Qdrant client");
                None
            }
        }
    }

    async fn ensure_collection(&self) -> Result<()> {
        let collections = self.client.list_collections().await?;
        let exists = collections
            .collections
            .iter()
            .any(|c| c.name == self.collection_name);

        if exists {
            info!(collection = %self.collection_name, "Qdrant collection exists");
            self.ensure_indexes().await?;
            return Ok(());
        }

        let mut vectors_config = VectorsConfigBuilder::default();
        vectors_config.add_named_vector_params(
            DENSE_VECTOR_NAME,
            VectorParamsBuilder::new(DENSE_VECTOR_SIZE, Distance::Cosine),
        );

        let mut sparse_vectors_config = SparseVectorsConfigBuilder::default();
        sparse_vectors_config.add_named_vector_params(SPARSE_VECTOR_NAME, SparseVectorParamsBuilder::default());

        self.client
            .create_collection(
                CreateCollectionBuilder::new(&self.collection_name)
                    .vectors_config(VectorsConfig::from(vectors_config))
                    .sparse_vectors_config(SparseVectorConfig::from(sparse_vectors_config)),
            )
            .await
            .context("failed to create Qdrant collection")?;

        self.ensure_indexes().await?;

        info!(collection = %self.collection_name, "created Qdrant collection with indexes");
        Ok(())
    }

    async fn ensure_indexes(&self) -> Result<()> {
        for field in ["user_id", "meeting_id"] {
            let result = self
                .client
                .create_field_index(
                    CreateFieldIndexCollectionBuilder::new(
                        &self.collection_name,
                        field,
                        FieldType::Keyword,
                    ),
                )
                .await;
            // Ignore "already exists" errors
            if let Err(e) = &result {
                let msg = e.to_string();
                if !msg.contains("already exists") {
                    result.with_context(|| format!("failed to create index on {}", field))?;
                }
            }
        }
        Ok(())
    }

    pub async fn upsert_chunks(&self, chunks: Vec<ChunkPoint>) -> Result<()> {
        let count = chunks.len();
        let points: Vec<PointStruct> = chunks
            .into_iter()
            .map(|chunk| {
                let payload: std::collections::HashMap<String, QdrantValue> = [
                    ("meeting_id".to_owned(), QdrantValue::from(chunk.meeting_id)),
                    ("meeting_title".to_owned(), QdrantValue::from(chunk.meeting_title)),
                    ("user_id".to_owned(), QdrantValue::from(chunk.user_id)),
                    ("chunk_index".to_owned(), QdrantValue::from(chunk.chunk_index as i64)),
                    ("text".to_owned(), QdrantValue::from(chunk.text)),
                    ("start_ms".to_owned(), QdrantValue::from(chunk.start_ms)),
                    ("end_ms".to_owned(), QdrantValue::from(chunk.end_ms)),
                    ("speaker_label".to_owned(), QdrantValue::from(chunk.speaker_label.unwrap_or_default())),
                ]
                .into();

                let vectors: std::collections::HashMap<String, qdrant_client::qdrant::Vector> = [(
                    DENSE_VECTOR_NAME.to_owned(),
                    chunk.dense_vector.into(),
                )]
                .into();

                PointStruct::new(chunk.id, vectors, payload)
            })
            .collect();

        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection_name, points))
            .await
            .map_err(|e| {
                warn!(error = %e, collection = %self.collection_name, "Qdrant upsert failed");
                e
            })
            .context("failed to upsert points to Qdrant")?;

        info!(count = count, collection = %self.collection_name, "upserted chunks to Qdrant");
        Ok(())
    }

    pub async fn hybrid_search(
        &self,
        query_vector: Vec<f32>,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let mut builder = QueryPointsBuilder::new(&self.collection_name)
            .add_prefetch(
                PrefetchQueryBuilder::default()
                    .query(Query::new_nearest(query_vector))
                    .using(DENSE_VECTOR_NAME)
                    .limit(25u64),
            )
            .query(Query::new_fusion(Fusion::Rrf))
            .limit(limit as u64)
            .with_payload(true);

        if let Some(uid) = user_id {
            builder = builder.filter(Filter::must([Condition::matches(
                "user_id",
                uid.to_owned(),
            )]));
        }

        info!(
            collection = %self.collection_name,
            user_filter = ?user_id,
            limit = limit,
            "executing Qdrant hybrid search"
        );

        let response = self
            .client
            .query(builder)
            .await
            .map_err(|e| {
                warn!(error = %e, "Qdrant query failed");
                e
            })
            .context("Qdrant hybrid search failed")?;

        let results = response
            .result
            .into_iter()
            .filter_map(|point| {
                let payload = point.payload;
                Some(SearchResult {
                    meeting_id: payload.get("meeting_id")?.as_str()?.to_owned(),
                    meeting_title: payload.get("meeting_title").map(|v| v.to_string()).unwrap_or_default(),
                    text: payload.get("text")?.as_str()?.to_owned(),
                    start_ms: payload.get("start_ms")?.as_integer()?,
                    end_ms: payload.get("end_ms")?.as_integer()?,
                    speaker_label: payload
                        .get("speaker_label")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_owned()),
                })
            })
            .collect();

        Ok(results)
    }

    pub async fn delete_meeting_chunks(&self, meeting_id: &str) -> Result<()> {
        self.client
            .delete_points(
                DeletePointsBuilder::new(&self.collection_name)
                    .points(Filter::must([Condition::matches(
                        "meeting_id",
                        meeting_id.to_owned(),
                    )]))
                    .wait(true),
            )
            .await
            .context("failed to delete meeting chunks from Qdrant")?;

        info!(meeting_id = %meeting_id, "deleted chunks from Qdrant");
        Ok(())
    }
}
