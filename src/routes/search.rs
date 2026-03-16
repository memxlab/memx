use crate::db::{enhanced_search, track_retrievals};
use crate::error::Result;
use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use super::memory::AppState;

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

    // Generate the query embedding
    let query_embedding = state.embed_client.embed(&query.q).await?;

    // Execute enhanced search
    let conn = state.db.get().await;
    let mut results = enhanced_search(
        &conn,
        &query.q,
        &query_embedding,
        limit,
        state.search_options.clone(),
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

    Ok(Json(results))
}
