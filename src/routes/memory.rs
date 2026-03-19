use crate::db::{delete_memory, get_memory, track_access, MemoryInput};
use crate::error::Result;
use crate::service::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

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
    let memory = state.memx.add_memory(input).await?;

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
    let conn = state.memx.db().get().await;

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
    let memory = state.memx.update_memory(&id, input).await?;
    Ok(Json(memory))
}

async fn delete_memory_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.memx.db().get().await;
    delete_memory(&conn, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_memories_handler(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse> {
    let memories = state.memx.list_memories(query.limit, query.offset).await?;
    Ok(Json(memories))
}
