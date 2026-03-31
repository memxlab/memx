use crate::{
    db::{Memory, MemoryInput, MemoryType},
    error::AppError,
    service::MemxService,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars,
    schemars::JsonSchema,
    tool, tool_handler, tool_router, Json, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct MemxMcpServer {
    memx: MemxService,
    tool_router: ToolRouter<Self>,
}

impl MemxMcpServer {
    pub fn new(memx: MemxService) -> Self {
        Self {
            memx,
            tool_router: Self::tool_router(),
        }
    }

    fn instructions() -> &'static str {
        "Use MemX to store, search, inspect, update, list, and delete long-term memories."
    }

    fn normalize_limit(limit: Option<usize>, default: usize, max: usize) -> usize {
        limit.unwrap_or(default).max(1).min(max)
    }

    fn app_error_to_string(err: AppError) -> String {
        match err {
            AppError::Database(e) => format!("Database error: {e}"),
            AppError::NotFound(msg) => msg,
            AppError::Embedding(msg) => format!("Embedding error: {msg}"),
            AppError::Internal(msg) => msg,
            AppError::Anyhow(e) => e.to_string(),
        }
    }

    async fn add_memory_impl(
        &self,
        request: AddMemoryRequest,
        embedding_override: Option<Vec<f32>>,
    ) -> Result<AddMemoryResponse, String> {
        let input = MemoryInput {
            content: request.content,
            memory_type: request.memory_type,
            tags: request.tags,
            metadata: request.metadata,
            importance: request.importance,
        };

        let memory = match embedding_override {
            Some(embedding) => self
                .memx
                .add_memory_with_embedding(input, embedding)
                .await
                .map_err(Self::app_error_to_string)?,
            None => self
                .memx
                .add_memory(input)
                .await
                .map_err(Self::app_error_to_string)?,
        };

        Ok(AddMemoryResponse { memory })
    }

    async fn search_memories_impl(
        &self,
        request: SearchMemoriesRequest,
        embedding_override: Option<Vec<f32>>,
    ) -> Result<SearchMemoriesResponse, String> {
        let limit = Self::normalize_limit(request.limit, 10, 50);
        let memories = match embedding_override {
            Some(embedding) => self
                .memx
                .search_memories_with_embedding(&request.query, &embedding, limit)
                .await
                .map_err(Self::app_error_to_string)?,
            None => self
                .memx
                .search_memories(&request.query, limit)
                .await
                .map_err(Self::app_error_to_string)?,
        };

        Ok(SearchMemoriesResponse { memories })
    }

    async fn update_memory_impl(
        &self,
        request: UpdateMemoryRequest,
        embedding_override: Option<Vec<f32>>,
    ) -> Result<UpdateMemoryResponse, String> {
        if request.content.is_none() && request.importance.is_none() {
            return Err("Provide at least one field to update: content or importance.".to_string());
        }

        let input = MemoryInput {
            content: request.content.unwrap_or_default(),
            memory_type: None,
            tags: None,
            metadata: None,
            importance: request.importance,
        };

        let memory = self
            .memx
            .update_memory_with_embedding(&request.id, input, embedding_override)
            .await
            .map_err(Self::app_error_to_string)?;

        Ok(UpdateMemoryResponse { memory })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MemxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(Self::instructions())
    }
}

#[tool_router]
impl MemxMcpServer {
    #[tool(
        name = "add-memory",
        description = "Store a new memory in MemX with optional type, tags, metadata, and importance."
    )]
    async fn add_memory(
        &self,
        Parameters(request): Parameters<AddMemoryRequest>,
    ) -> Result<Json<AddMemoryResponse>, String> {
        self.add_memory_impl(request, None).await.map(Json)
    }

    #[tool(
        name = "search-memories",
        description = "Search MemX memories by semantic query. Use this first when trying to recall information."
    )]
    async fn search_memories(
        &self,
        Parameters(request): Parameters<SearchMemoriesRequest>,
    ) -> Result<Json<SearchMemoriesResponse>, String> {
        self.search_memories_impl(request, None).await.map(Json)
    }

    #[tool(name = "get-memory", description = "Read a single memory by ID.")]
    async fn get_memory(
        &self,
        Parameters(request): Parameters<GetMemoryRequest>,
    ) -> Result<Json<GetMemoryResponse>, String> {
        let memory = self
            .memx
            .get_memory(&request.id)
            .await
            .map_err(Self::app_error_to_string)?;
        Ok(Json(GetMemoryResponse { memory }))
    }

    #[tool(
        name = "update-memory",
        description = "Update an existing memory. Current support is content and importance."
    )]
    async fn update_memory(
        &self,
        Parameters(request): Parameters<UpdateMemoryRequest>,
    ) -> Result<Json<UpdateMemoryResponse>, String> {
        self.update_memory_impl(request, None).await.map(Json)
    }

    #[tool(name = "delete-memory", description = "Delete a memory by ID.")]
    async fn delete_memory(
        &self,
        Parameters(request): Parameters<DeleteMemoryRequest>,
    ) -> Result<Json<DeleteMemoryResponse>, String> {
        self.memx
            .delete_memory(&request.id)
            .await
            .map_err(Self::app_error_to_string)?;
        Ok(Json(DeleteMemoryResponse {
            id: request.id,
            deleted: true,
        }))
    }

    #[tool(
        name = "list-memories",
        description = "List recent memories with optional pagination."
    )]
    async fn list_memories(
        &self,
        Parameters(request): Parameters<ListMemoriesRequest>,
    ) -> Result<Json<ListMemoriesResponse>, String> {
        let memories = self
            .memx
            .list_memories(request.limit, request.offset)
            .await
            .map_err(Self::app_error_to_string)?;
        Ok(Json(ListMemoriesResponse { memories }))
    }
}

pub async fn serve_stdio(memx: MemxService) -> anyhow::Result<()> {
    let server = MemxMcpServer::new(memx);
    server
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?
        .waiting()
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AddMemoryRequest {
    pub content: String,
    #[serde(rename = "type")]
    pub memory_type: Option<MemoryType>,
    pub tags: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
    pub importance: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchMemoriesRequest {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetMemoryRequest {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UpdateMemoryRequest {
    pub id: String,
    pub content: Option<String>,
    pub importance: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DeleteMemoryRequest {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ListMemoriesRequest {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AddMemoryResponse {
    pub memory: Memory,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchMemoriesResponse {
    pub memories: Vec<Memory>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GetMemoryResponse {
    pub memory: Memory,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UpdateMemoryResponse {
    pub memory: Memory,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DeleteMemoryResponse {
    pub id: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ListMemoriesResponse {
    pub memories: Vec<Memory>,
}

#[cfg(test)]
mod tests {
    use super::{AddMemoryRequest, MemxMcpServer, SearchMemoriesRequest, UpdateMemoryRequest};
    use crate::{
        db::{
            init_schema,
            test_utils::{cleanup, open_test_db},
            DbPool, SearchOptions,
        },
        embed::EmbedClient,
        service::MemxService,
    };
    use anyhow::Result;
    use axum::{routing::post, Json as AxumJson, Router};
    use rmcp::{model::CallToolRequestParams, ServiceExt};
    use serde_json::json;

    async fn mock_embedding_handler(
        AxumJson(payload): AxumJson<serde_json::Value>,
    ) -> AxumJson<serde_json::Value> {
        let input = payload
            .get("input")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        let embedding = if input.contains("rust") {
            vec![1.0_f32, 0.0, 0.0]
        } else if input.contains("coffee") {
            vec![0.0_f32, 1.0, 0.0]
        } else {
            vec![0.0_f32, 0.0, 1.0]
        };

        AxumJson(json!({
            "data": [
                { "embedding": embedding }
            ]
        }))
    }

    async fn spawn_mock_embed_server() -> Result<(String, tokio::task::JoinHandle<()>)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let app = Router::new().route("/embeddings", post(mock_embedding_handler));
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok((format!("http://{}", addr), handle))
    }

    async fn test_service(
        name: &str,
    ) -> Result<(MemxService, std::path::PathBuf, tokio::task::JoinHandle<()>)> {
        let (conn, path) = open_test_db(name).await;
        init_schema(&conn, 3).await?;
        drop(conn);

        let db = DbPool::new(path.to_str().unwrap()).await?;
        let (base_url, handle) = spawn_mock_embed_server().await?;
        let embed_client =
            EmbedClient::new("test".to_string(), base_url, "test-model".to_string(), 3);

        Ok((
            MemxService::new(db, embed_client, SearchOptions::default()),
            path,
            handle,
        ))
    }

    #[tokio::test]
    async fn mcp_tool_impl_round_trip_works() -> Result<()> {
        let (memx, path, embed_handle) = test_service("mcp_tool_impl_round_trip").await?;
        let server = MemxMcpServer::new(memx);

        let created = server
            .add_memory_impl(
                AddMemoryRequest {
                    content: "rust memory".to_string(),
                    memory_type: None,
                    tags: Some(vec!["dev".to_string()]),
                    metadata: None,
                    importance: Some(0.8),
                },
                Some(vec![1.0, 0.0, 0.0]),
            )
            .await
            .map_err(anyhow::Error::msg)?;

        let found = server
            .search_memories_impl(
                SearchMemoriesRequest {
                    query: "rust".to_string(),
                    limit: Some(5),
                },
                Some(vec![1.0, 0.0, 0.0]),
            )
            .await
            .map_err(anyhow::Error::msg)?;

        let updated = server
            .update_memory_impl(
                UpdateMemoryRequest {
                    id: created.memory.id.clone(),
                    content: Some("rust memory updated".to_string()),
                    importance: Some(0.9),
                },
                Some(vec![1.0, 0.0, 0.0]),
            )
            .await
            .map_err(anyhow::Error::msg)?;

        assert_eq!(found.memories.len(), 1);
        assert_eq!(found.memories[0].id, created.memory.id);
        assert_eq!(updated.memory.content, "rust memory updated");
        assert_eq!(updated.memory.importance, 0.9);

        cleanup(path);
        embed_handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn mcp_protocol_lists_tools_and_handles_memory_flow() -> Result<()> {
        let (memx, path, embed_handle) = test_service("mcp_protocol_memory_flow").await?;
        let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);

        let server_handle = tokio::spawn(async move {
            MemxMcpServer::new(memx)
                .serve(server_transport)
                .await?
                .waiting()
                .await?;
            anyhow::Ok(())
        });

        let client = ().serve(client_transport).await?;
        let tools = client.peer().list_all_tools().await?;
        let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();

        assert!(tool_names.contains(&"add-memory"));
        assert!(tool_names.contains(&"search-memories"));
        assert!(tool_names.contains(&"get-memory"));
        assert!(tool_names.contains(&"update-memory"));
        assert!(tool_names.contains(&"delete-memory"));
        assert!(tool_names.contains(&"list-memories"));

        let created = client
            .call_tool(
                CallToolRequestParams::new("add-memory").with_arguments(
                    json!({
                        "content": "coffee note",
                        "type": "semantic",
                        "tags": ["drink"],
                        "importance": 0.6
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;

        let created_value = created
            .structured_content
            .expect("expected structured add-memory output");
        let created_id = created_value["memory"]["id"]
            .as_str()
            .expect("expected memory id")
            .to_string();

        let found = client
            .call_tool(
                CallToolRequestParams::new("search-memories").with_arguments(
                    json!({
                        "query": "coffee",
                        "limit": 5
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;
        let found_value = found
            .structured_content
            .expect("expected structured search output");
        assert_eq!(found_value["memories"].as_array().unwrap().len(), 1);

        let fetched = client
            .call_tool(
                CallToolRequestParams::new("get-memory").with_arguments(
                    json!({ "id": created_id.clone() })
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await?;
        let fetched_value = fetched
            .structured_content
            .expect("expected structured get output");
        assert_eq!(
            fetched_value["memory"]["id"].as_str(),
            Some(created_id.as_str())
        );

        let deleted = client
            .call_tool(
                CallToolRequestParams::new("delete-memory")
                    .with_arguments(json!({ "id": created_id }).as_object().unwrap().clone()),
            )
            .await?;
        let deleted_value = deleted
            .structured_content
            .expect("expected structured delete output");
        assert_eq!(deleted_value["deleted"].as_bool(), Some(true));

        client.cancel().await?;
        server_handle.await??;
        cleanup(path);
        embed_handle.abort();
        Ok(())
    }
}
