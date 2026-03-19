use crate::{
    db::{
        create_memory, enhanced_search, list_memories as db_list_memories, track_retrievals,
        update_memory as db_update_memory, DbPool, Memory, MemoryInput, SearchOptions,
    },
    embed::EmbedClient,
    error::Result,
};

#[derive(Clone)]
pub struct MemxService {
    db: DbPool,
    embed_client: EmbedClient,
    search_options: SearchOptions,
}

#[derive(Clone)]
pub struct AppState {
    pub memx: MemxService,
}

impl AppState {
    pub fn new(memx: MemxService) -> Self {
        Self { memx }
    }
}

impl MemxService {
    pub fn new(db: DbPool, embed_client: EmbedClient, search_options: SearchOptions) -> Self {
        Self {
            db,
            embed_client,
            search_options,
        }
    }

    pub fn db(&self) -> &DbPool {
        &self.db
    }

    pub fn embed_client(&self) -> &EmbedClient {
        &self.embed_client
    }

    pub async fn add_memory(&self, input: MemoryInput) -> Result<Memory> {
        let embedding = self.embed_client.embed(&input.content).await?;
        self.add_memory_with_embedding(input, embedding).await
    }

    pub(crate) async fn add_memory_with_embedding(
        &self,
        input: MemoryInput,
        embedding: Vec<f32>,
    ) -> Result<Memory> {
        let conn = self.db.get().await;
        create_memory(&conn, input, embedding).await
    }

    pub async fn search_memories(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let query_embedding = self.embed_client.embed(query).await?;
        self.search_memories_with_embedding(query, &query_embedding, limit)
            .await
    }

    // NOTE: This method holds the DB mutex across both the search query and the
    // retrieval tracking update. Since DbPool wraps a single Arc<Mutex<Connection>>,
    // concurrent requests (REST + MCP) will serialize here. Acceptable for the
    // current single-user local deployment; revisit if connection pooling is added.
    pub(crate) async fn search_memories_with_embedding(
        &self,
        query: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let conn = self.db.get().await;
        let mut results = enhanced_search(
            &conn,
            query,
            query_embedding,
            limit,
            self.search_options.clone(),
        )
        .await?;

        let ids: Vec<String> = results.iter().map(|memory| memory.id.clone()).collect();
        if !ids.is_empty() {
            match track_retrievals(&conn, &ids).await {
                Ok(tracked_at) => {
                    for memory in &mut results {
                        memory.retrieval_count += 1;
                        memory.last_retrieved_at = Some(tracked_at);
                    }
                }
                Err(err) => {
                    tracing::warn!("Failed to track retrieval stats: {}", err);
                }
            }
        }

        Ok(results)
    }

    pub async fn update_memory(&self, id: &str, input: MemoryInput) -> Result<Memory> {
        let (content, embedding) = if input.content.is_empty() {
            (None, None)
        } else {
            let emb = self.embed_client.embed(&input.content).await?;
            (Some(input.content), Some(emb))
        };

        let conn = self.db.get().await;
        db_update_memory(&conn, id, content, embedding, input.importance).await
    }

    pub async fn list_memories(
        &self,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<Memory>> {
        let conn = self.db.get().await;
        db_list_memories(&conn, limit, offset).await
    }
}

#[cfg(test)]
mod tests {
    use super::MemxService;
    use crate::db::test_utils::{cleanup, open_test_db};
    use crate::{
        db::{create_memory, init_schema, DbPool, MemoryInput, MemoryType, SearchOptions},
        embed::EmbedClient,
    };

    fn test_service(db: crate::db::DbPool) -> MemxService {
        MemxService::new(
            db,
            EmbedClient::new(
                "test".to_string(),
                "http://127.0.0.1:9".to_string(),
                "test-model".to_string(),
                3,
            ),
            SearchOptions::default(),
        )
    }

    #[tokio::test]
    async fn add_memory_with_embedding_applies_default_type_and_clamps_importance() {
        let (conn, path) = open_test_db("service_add_memory_defaults").await;
        {
            init_schema(&conn, 3).await.unwrap();
        }
        drop(conn);

        let db = DbPool::new(path.to_str().unwrap()).await.unwrap();

        let service = test_service(db.clone());
        let memory = service
            .add_memory_with_embedding(
                MemoryInput {
                    content: "keep this".to_string(),
                    memory_type: None,
                    tags: None,
                    metadata: None,
                    importance: Some(2.0),
                },
                vec![1.0, 0.0, 0.0],
            )
            .await
            .unwrap();

        assert!(matches!(memory.memory_type, MemoryType::Semantic));
        assert_eq!(memory.importance, 1.0);

        cleanup(path);
    }

    #[tokio::test]
    async fn search_memories_with_embedding_updates_retrieval_stats() {
        let (conn, path) = open_test_db("service_search_retrieval_stats").await;
        {
            init_schema(&conn, 3).await.unwrap();

            create_memory(
                &conn,
                MemoryInput {
                    content: "rust notes".to_string(),
                    memory_type: None,
                    tags: None,
                    metadata: None,
                    importance: None,
                },
                vec![1.0, 0.0, 0.0],
            )
            .await
            .unwrap();
            create_memory(
                &conn,
                MemoryInput {
                    content: "coffee notes".to_string(),
                    memory_type: None,
                    tags: None,
                    metadata: None,
                    importance: None,
                },
                vec![0.0, 1.0, 0.0],
            )
            .await
            .unwrap();
        }

        drop(conn);
        let db = DbPool::new(path.to_str().unwrap()).await.unwrap();

        let service = test_service(db.clone());
        let results = service
            .search_memories_with_embedding("rust", &[1.0, 0.0, 0.0], 10)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "rust notes");
        assert_eq!(results[0].retrieval_count, 1);
        assert!(results[0].last_retrieved_at.is_some());

        cleanup(path);
    }

    #[tokio::test]
    async fn list_memories_returns_recent_first_and_respects_offset() {
        let (conn, path) = open_test_db("service_list_recent_order").await;
        let first_id;
        let second_id;

        {
            init_schema(&conn, 3).await.unwrap();

            first_id = create_memory(
                &conn,
                MemoryInput {
                    content: "first".to_string(),
                    memory_type: None,
                    tags: None,
                    metadata: None,
                    importance: None,
                },
                vec![1.0, 0.0, 0.0],
            )
            .await
            .unwrap()
            .id;
            second_id = create_memory(
                &conn,
                MemoryInput {
                    content: "second".to_string(),
                    memory_type: None,
                    tags: None,
                    metadata: None,
                    importance: None,
                },
                vec![0.0, 1.0, 0.0],
            )
            .await
            .unwrap()
            .id;

            conn.execute(
                "UPDATE memories SET created_at = ?, updated_at = ? WHERE id = ?",
                libsql::params![10_i64, 10_i64, first_id.clone()],
            )
            .await
            .unwrap();
            conn.execute(
                "UPDATE memories SET created_at = ?, updated_at = ? WHERE id = ?",
                libsql::params![20_i64, 20_i64, second_id.clone()],
            )
            .await
            .unwrap();
        }

        drop(conn);
        let db = DbPool::new(path.to_str().unwrap()).await.unwrap();

        let service = test_service(db.clone());
        let memories = service.list_memories(Some(1), Some(1)).await.unwrap();

        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].id, first_id);
        assert_ne!(memories[0].id, second_id);

        cleanup(path);
    }
}
