use crate::error::Result;
use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use super::AppState;

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    limit: Option<usize>,
}

pub fn search_routes() -> Router<AppState> {
    Router::new().route("/memories/search", get(search_handler))
}

async fn search_handler(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<impl IntoResponse> {
    let limit = query.limit.unwrap_or(10).min(50);
    let results = state.memx.search_memories(&query.q, limit).await?;

    Ok(Json(results))
}
