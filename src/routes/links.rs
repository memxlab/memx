use crate::db::{create_link, delete_link, get_link_count, get_links, LinkInput};
use crate::error::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};

use super::AppState;

pub fn link_routes() -> Router<AppState> {
    Router::new()
        .route("/memories/{id}/links", post(create_link_handler))
        .route("/memories/{id}/links", get(get_links_handler))
        .route("/memories/links/{link_id}", delete(delete_link_handler))
        .route("/memories/{id}/link-count", get(get_link_count_handler))
}

async fn create_link_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut input): Json<LinkInput>,
) -> Result<impl IntoResponse> {
    // Ensure source_id matches the path parameter
    input.source_id = id;

    let conn = state.memx.db().get().await;
    let link = create_link(&conn, input).await?;

    Ok((StatusCode::CREATED, Json(link)))
}

async fn get_links_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.memx.db().get().await;
    let links = get_links(&conn, &id, true).await?;
    Ok(Json(links))
}

async fn delete_link_handler(
    State(state): State<AppState>,
    Path(link_id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.memx.db().get().await;
    delete_link(&conn, &link_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_link_count_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let conn = state.memx.db().get().await;
    let count = get_link_count(&conn, &id).await?;
    Ok(Json(serde_json::json!({ "count": count })))
}
