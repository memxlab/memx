use crate::db::{
    create_memory, delete_memory, get_memory, list_memories, track_access, update_memory, DbPool,
    MemoryInput, SearchOptions,
};
use crate::embed::EmbedClient;
use crate::error::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub embed_client: EmbedClient,
    pub search_options: SearchOptions,
}

#[derive(Deserialize)]
pub struct ListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Serialize)]
pub struct CreateResponse {
    pub id: String,
    pub content: String,
    #[serde(rename = "type")]
    pub memory_type: String,
    pub tags: Vec<String>,
    pub importance: f64,
    pub created_at: i64,
}

pub fn memory_routes() -> Router<AppState> {
    Router::new()
        .route("/memories", post(create_memory_handler))
        .route("/memories", get(list_memories_handler))
        .route("/memories/{id}", get(get_memory_handler))
        .route("/memories/{id}", put(update_memory_handler))
        .route("/memories/{id}", delete(delete_memory_handler))
}

async fn create_memory_handler(
    State(state): State<AppState>,
    Json(input): Json<MemoryInput>,
) -> Result<impl IntoResponse> {
    // Generate the embedding
    let embedding = state.embed_client.embed(&input.content).await?;

    // Create the memory
    let conn = state.db.get().await;
    let memory = create_memory(&conn, input, embedding).await?;

    let response = CreateResponse {
        id: memory.id,
        content: memory.content,
        memory_type: memory.memory_type.as_str().to_string(),
        tags: memory.tags,
        importance: memory.importance,
        created_at: memory.created_at,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

async fn get_memory_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.db.get().await;

    // Track access
    if let Err(err) = track_access(&conn, &id).await {
        tracing::warn!("Failed to track access stats: {}", err);
    }

    let memory = get_memory(&conn, &id).await?;
    Ok(Json(memory))
}

async fn update_memory_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<MemoryInput>,
) -> Result<impl IntoResponse> {
    let conn = state.db.get().await;

    // Recompute the embedding if the content changed
    let embedding = if input.content.is_empty() {
        None
    } else {
        Some(state.embed_client.embed(&input.content).await?)
    };

    let content = if input.content.is_empty() {
        None
    } else {
        Some(input.content)
    };

    let memory = update_memory(&conn, &id, content, embedding, input.importance).await?;
    Ok(Json(memory))
}

async fn delete_memory_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.db.get().await;
    delete_memory(&conn, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_memories_handler(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse> {
    let conn = state.db.get().await;
    let memories = list_memories(&conn, query.limit, query.offset).await?;
    Ok(Json(memories))
}
