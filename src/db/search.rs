use crate::db::types::{Memory, SearchOptions};
use crate::error::{AppError, Result};
use chrono::Utc;
use libsql::{params, Connection};

const MAX_RESULTS_PER_TAG_SIGNATURE: usize = 1;

pub async fn vector_search(
    conn: &Connection,
    query_embedding: &[f32],
    limit: usize,
    threshold: f64,
) -> Result<Vec<Memory>> {
    let embedding_json = serde_json::to_string(query_embedding)
        .map_err(|e| AppError::Internal(format!("Failed to serialize embedding: {}", e)))?;

    let distance_threshold = 1.0 - threshold;

    // Prefer the DiskANN vector index (O(log n)) and fall back to brute-force search (O(n)) on failure
    match vector_search_ann(conn, &embedding_json, limit, distance_threshold).await {
        Ok(memories) => Ok(memories),
        Err(e) => {
            tracing::warn!("ANN search failed, falling back to brute-force: {}", e);
            vector_search_brute(conn, &embedding_json, limit, distance_threshold).await
        }
    }
}

/// ANN search using DiskANN vector index (vector_top_k)
async fn vector_search_ann(
    conn: &Connection,
    embedding_json: &str,
    limit: usize,
    distance_threshold: f64,
) -> Result<Vec<Memory>> {
    // vector_top_k returns (id, distance) - join to get full row
    // Use vector32(?) to convert JSON string to F32_BLOB for same-type matching
    let stmt = conn
        .prepare(
            r#"
            SELECT
                m.id, m.content, m.type, m.tags, m.metadata, m.importance,
                m.access_count, m.last_accessed_at, m.retrieval_count, m.last_retrieved_at,
                m.created_at, m.updated_at,
                vector_distance_cos(m.embedding, vector32(?)) AS distance
            FROM vector_top_k('idx_vec_memories', vector32(?), ?) AS vt
            JOIN memories AS m ON m.rowid = vt.id
            ORDER BY distance ASC
            "#,
        )
        .await?;

    let mut rows = stmt
        .query(params![embedding_json, embedding_json, limit as i64])
        .await?;

    let mut memories = Vec::new();

    while let Some(row) = rows.next().await? {
        let distance: f64 = row.get(12)?;
        if distance >= distance_threshold {
            continue;
        }
        let mut memory = super::memory::parse_memory_row(&row)?;
        memory.score = Some(1.0 - distance);
        memories.push(memory);
    }

    Ok(memories)
}

/// Brute-force search using vector_distance_cos (fallback when DiskANN index unavailable)
async fn vector_search_brute(
    conn: &Connection,
    embedding_json: &str,
    limit: usize,
    distance_threshold: f64,
) -> Result<Vec<Memory>> {
    let stmt = conn
        .prepare(
            r#"
            SELECT
                id, content, type, tags, metadata, importance,
                access_count, last_accessed_at, retrieval_count, last_retrieved_at,
                created_at, updated_at,
                vector_distance_cos(embedding, vector32(?)) AS distance
            FROM memories
            WHERE embedding IS NOT NULL
              AND vector_distance_cos(embedding, vector32(?)) < ?
            ORDER BY distance ASC
            LIMIT ?
            "#,
        )
        .await?;

    let mut rows = stmt
        .query(params![
            embedding_json,
            embedding_json,
            distance_threshold,
            limit as i64
        ])
        .await?;

    let mut memories = Vec::new();

    while let Some(row) = rows.next().await? {
        let mut memory = super::memory::parse_memory_row(&row)?;
        let distance: f64 = row.get(12)?;
        memory.score = Some(1.0 - distance);
        memories.push(memory);
    }

    Ok(memories)
}

/// Build a FTS5 MATCH query with mixed AND/OR logic:
///
///   - CJK characters use OR (partial match is useful for ideographs)
///   - Non-CJK words use implicit AND (all words should be present)
///
/// Examples:
///   "我是谁"              → `("我" OR "是" OR "谁")`
///   "database"            → `"database"`
///   "hello world"         → `"hello" "world"`  (implicit AND)
///   "hello 你好 world"    → `"hello" ("你" OR "好") "world"`
///   "test你好world"       → `"test" ("你" OR "好") "world"`
fn build_fts_query(query: &str) -> String {
    use super::fts::is_cjk;

    let mut parts: Vec<String> = Vec::new();

    for word in query.split_whitespace() {
        if word.chars().any(is_cjk) {
            // Mixed or pure CJK word: accumulate non-CJK runs as whole tokens,
            // emit each CJK char individually, then group CJK chars with OR.
            let mut ascii_buf = String::new();
            let mut cjk_chars: Vec<char> = Vec::new();

            for ch in word.chars() {
                if is_cjk(ch) {
                    if !ascii_buf.is_empty() {
                        parts.push(format!("\"{}\"", ascii_buf.replace('"', "\"\"")));
                        ascii_buf.clear();
                    }
                    cjk_chars.push(ch);
                } else {
                    if !cjk_chars.is_empty() {
                        parts.push(cjk_or_group(&cjk_chars));
                        cjk_chars.clear();
                    }
                    ascii_buf.push(ch);
                }
            }

            if !ascii_buf.is_empty() {
                parts.push(format!("\"{}\"", ascii_buf.replace('"', "\"\"")));
            }
            if !cjk_chars.is_empty() {
                parts.push(cjk_or_group(&cjk_chars));
            }
        } else {
            // Pure non-CJK word: emit as AND term (implicit AND between parts)
            parts.push(format!("\"{}\"", word.replace('"', "\"\"")));
        }
    }

    if parts.is_empty() {
        return format!("\"{}\"", query.replace('"', "\"\""));
    }

    parts.join(" ")
}

/// Wrap CJK characters in an OR group: `("我" OR "是" OR "谁")`
fn cjk_or_group(chars: &[char]) -> String {
    if chars.len() == 1 {
        return format!("\"{}\"", chars[0]);
    }
    let inner: Vec<String> = chars.iter().map(|c| format!("\"{}\"", c)).collect();
    format!("({})", inner.join(" OR "))
}

pub async fn keyword_search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<Memory>> {
    let stmt = conn
        .prepare(
            r#"
            SELECT
                m.id, m.content, m.type, m.tags, m.metadata, m.importance,
                m.access_count, m.last_accessed_at, m.retrieval_count, m.last_retrieved_at,
                m.created_at, m.updated_at
            FROM memories_fts f
            JOIN memories m ON m.rowid = f.rowid
            WHERE memories_fts MATCH ?
            ORDER BY rank
            LIMIT ?
            "#,
        )
        .await?;

    let fts_query = build_fts_query(query);
    let mut rows = stmt.query(params![fts_query, limit as i64]).await?;
    let mut memories = Vec::new();

    while let Some(row) = rows.next().await? {
        memories.push(super::memory::parse_memory_row(&row)?);
    }

    Ok(memories)
}

fn enhanced_search_inner(
    vec_results: Vec<Memory>,
    kw_results: Vec<Memory>,
    limit: usize,
    options: SearchOptions,
) -> Result<Vec<Memory>> {
    let top_vector_score = vec_results.first().and_then(|memory| memory.score);
    let has_keyword_hits = !kw_results.is_empty();

    // Rejection rule (can be skipped via enable_rejection)
    if options.enable_rejection
        && !has_keyword_hits
        && top_vector_score.unwrap_or(0.0) < options.no_keyword_min_vector_score
    {
        return Ok(Vec::new());
    }

    // RRF merge (only runs when keyword results exist)
    let merged = if options.enable_keyword {
        rrf_merge(vec_results, kw_results)
    } else {
        vec_results
    };

    let dedup_enabled = options.enable_dedup;

    // Apply multi-dimensional scoring
    let scored = apply_multidim_scoring(merged, options);

    // Apply Z-score normalization
    let normalized = zscore_normalize(scored);

    // Sort and limit the result count
    let mut result = normalized;
    result.sort_by(|a, b| {
        b.final_score
            .unwrap_or(0.0)
            .partial_cmp(&a.final_score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Deduplicate (can be skipped via enable_dedup)
    if dedup_enabled {
        let mut deduped = Vec::with_capacity(limit);
        let mut seen_contents = std::collections::HashSet::new();
        let mut tag_signature_counts = std::collections::HashMap::new();

        for memory in result {
            let content_key = memory.content.trim().to_string();
            if !seen_contents.insert(content_key) {
                continue;
            }

            if let Some(signature) = tag_signature(&memory) {
                let count = tag_signature_counts.entry(signature).or_insert(0);
                if *count >= MAX_RESULTS_PER_TAG_SIGNATURE {
                    continue;
                }
                *count += 1;
            }

            deduped.push(memory);
            if deduped.len() == limit {
                break;
            }
        }

        Ok(deduped)
    } else {
        result.truncate(limit);
        Ok(result)
    }
}

pub async fn enhanced_search(
    conn: &Connection,
    query: &str,
    query_embedding: &[f32],
    limit: usize,
    options: SearchOptions,
) -> Result<Vec<Memory>> {
    let vec_results = vector_search(conn, query_embedding, limit * 3, 0.3).await?;

    let kw_results = if options.enable_keyword {
        keyword_search(conn, query, limit * 3).await?
    } else {
        Vec::new()
    };

    enhanced_search_inner(vec_results, kw_results, limit, options)
}

fn tag_signature(memory: &Memory) -> Option<String> {
    if memory.tags.is_empty() {
        return None;
    }

    let mut tags = memory.tags.clone();
    tags.sort();
    Some(format!(
        "{}::{}",
        memory.memory_type.as_str(),
        tags.join("|")
    ))
}

fn rrf_merge(vec_results: Vec<Memory>, kw_results: Vec<Memory>) -> Vec<Memory> {
    use std::collections::HashMap;

    const RRF_K: f64 = 60.0;

    let mut rrf_scores: HashMap<String, f64> = HashMap::new();
    let mut memories: HashMap<String, Memory> = HashMap::new();

    // Vector search results (memory.score already holds the cosine similarity)
    for (rank, memory) in vec_results.into_iter().enumerate() {
        let score = 1.0 / (RRF_K + (rank + 1) as f64);
        *rrf_scores.entry(memory.id.clone()).or_insert(0.0) += score;
        memories.insert(memory.id.clone(), memory);
    }

    // Keyword search results (memory.score is None; prefer vec version if present)
    for (rank, memory) in kw_results.into_iter().enumerate() {
        let score = 1.0 / (RRF_K + (rank + 1) as f64);
        *rrf_scores.entry(memory.id.clone()).or_insert(0.0) += score;
        memories.entry(memory.id.clone()).or_insert(memory);
    }

    // Sort by RRF score (descending) but PRESERVE the original vector similarity
    // in memory.score so that apply_multidim_scoring uses actual semantic relevance.
    let mut result: Vec<Memory> = memories.into_values().collect();
    result.sort_by(|a, b| {
        let sa = rrf_scores.get(&a.id).unwrap_or(&0.0);
        let sb = rrf_scores.get(&b.id).unwrap_or(&0.0);
        sb.partial_cmp(sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    result
}

fn apply_multidim_scoring(memories: Vec<Memory>, opts: SearchOptions) -> Vec<Memory> {
    let now = Utc::now().timestamp();

    memories
        .into_iter()
        .map(|mut m| {
            let semantic = m.score.unwrap_or(0.0);
            let recency = calculate_recency_score(
                effective_recency_timestamp(&m),
                now,
                opts.decay_half_life_days,
            );
            let frequency = calculate_frequency_score(effective_frequency_count(&m));
            let importance = m.importance;

            m.final_score = Some(
                semantic * opts.semantic_weight
                    + recency * opts.recency_weight
                    + frequency * opts.frequency_weight
                    + importance * opts.importance_weight,
            );
            m
        })
        .collect()
}

fn effective_recency_timestamp(memory: &Memory) -> i64 {
    memory.last_retrieved_at.unwrap_or(memory.created_at)
}

fn effective_frequency_count(memory: &Memory) -> i64 {
    if memory.retrieval_count > 0 {
        memory.retrieval_count
    } else {
        memory.access_count
    }
}

fn calculate_recency_score(created_at: i64, now: i64, half_life_days: f64) -> f64 {
    let age_seconds = (now - created_at).max(0) as f64;
    let age_days = age_seconds / 86400.0;

    // Exponential decay: score = 2^(-age/half_life)
    2.0_f64.powf(-age_days / half_life_days)
}

fn calculate_frequency_score(access_count: i64) -> f64 {
    // Log normalization
    ((access_count as f64 + 1.0).ln() / 10.0).min(1.0)
}

fn zscore_normalize(mut memories: Vec<Memory>) -> Vec<Memory> {
    if memories.len() < 2 {
        return memories;
    }

    let scores: Vec<f64> = memories.iter().filter_map(|m| m.final_score).collect();

    if scores.is_empty() {
        return memories;
    }

    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
    let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / scores.len() as f64;
    let std_dev = variance.sqrt();

    if std_dev < 1e-6 {
        return memories;
    }

    for memory in &mut memories {
        if let Some(score) = memory.final_score {
            let z_score = (score - mean) / std_dev;
            // Convert to the 0-1 range using sigmoid
            memory.final_score = Some(1.0 / (1.0 + (-z_score).exp()));
        }
    }

    memories
}

#[cfg(test)]
mod tests {
    use super::{
        effective_frequency_count, effective_recency_timestamp, enhanced_search, keyword_search,
        vector_search, MAX_RESULTS_PER_TAG_SIGNATURE,
    };
    use crate::db::test_utils::{cleanup, open_test_db};
    use crate::db::{create_memory, init_schema, Memory, MemoryInput, MemoryType, SearchOptions};
    use libsql::params;
    use std::time::Instant;

    fn sample_memory() -> Memory {
        Memory {
            id: "memory-1".to_string(),
            content: "sample".to_string(),
            memory_type: MemoryType::Semantic,
            tags: Vec::new(),
            metadata: None,
            importance: 0.5,
            access_count: 0,
            last_accessed_at: None,
            retrieval_count: 0,
            last_retrieved_at: None,
            created_at: 100,
            updated_at: 100,
            score: None,
            final_score: None,
        }
    }

    #[test]
    fn recency_prefers_last_retrieved_at() {
        let mut memory = sample_memory();
        memory.created_at = 100;
        memory.last_retrieved_at = Some(200);

        assert_eq!(effective_recency_timestamp(&memory), 200);
    }

    #[test]
    fn recency_falls_back_to_created_at() {
        let memory = sample_memory();

        assert_eq!(effective_recency_timestamp(&memory), 100);
    }

    #[test]
    fn frequency_prefers_retrieval_count() {
        let mut memory = sample_memory();
        memory.access_count = 9;
        memory.retrieval_count = 2;

        assert_eq!(effective_frequency_count(&memory), 2);
    }

    #[test]
    fn frequency_falls_back_to_access_count() {
        let mut memory = sample_memory();
        memory.access_count = 4;

        assert_eq!(effective_frequency_count(&memory), 4);
    }

    #[tokio::test]
    async fn vector_search_returns_nearest_memories() {
        let (conn, path) = open_test_db("search").await;
        init_schema(&conn, 4).await.unwrap();

        let near = create_memory(
            &conn,
            MemoryInput {
                content: "nearest".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![1.0, 0.0, 0.0, 0.0],
        )
        .await
        .unwrap();

        let far = create_memory(
            &conn,
            MemoryInput {
                content: "far".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![0.6, 0.4, 0.0, 0.0],
        )
        .await
        .unwrap();

        let results = vector_search(&conn, &[1.0, 0.0, 0.0, 0.0], 2, 0.0)
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, near.id);
        assert_eq!(results[1].id, far.id);
        assert!(results[0].score.unwrap() >= results[1].score.unwrap());

        cleanup(path);
    }

    #[tokio::test]
    async fn vector_search_ann_works_with_high_dim_vectors() {
        let (conn, path) = open_test_db("search").await;
        // Use 1024 dimensions to test DiskANN (low dims like 4 don't support DiskANN)
        init_schema(&conn, 1024).await.unwrap();

        let mut near_vec = vec![0.0f32; 1024];
        near_vec[0] = 1.0;
        let near = create_memory(
            &conn,
            MemoryInput {
                content: "nearest".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            near_vec.clone(),
        )
        .await
        .unwrap();

        let mut far_vec = vec![0.0f32; 1024];
        far_vec[0] = 0.6;
        far_vec[500] = 0.8;
        create_memory(
            &conn,
            MemoryInput {
                content: "far".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            far_vec,
        )
        .await
        .unwrap();

        // DiskANN index should have been created by init_schema with F32_BLOB(1024)
        // vector_search should use ANN path (not brute-force fallback)
        let results = vector_search(&conn, &near_vec, 2, 0.0).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, near.id);
        assert!(results[0].score.unwrap() > results[1].score.unwrap());

        cleanup(path);
    }

    #[tokio::test]
    async fn keyword_search_matches_content_terms() {
        let (conn, path) = open_test_db("search").await;
        init_schema(&conn, 4).await.unwrap();

        let match_memory = create_memory(
            &conn,
            MemoryInput {
                content: "database performance tuning".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![0.1, 0.2, 0.3, 0.4],
        )
        .await
        .unwrap();

        create_memory(
            &conn,
            MemoryInput {
                content: "rust ownership basics".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![0.4, 0.3, 0.2, 0.1],
        )
        .await
        .unwrap();

        let results = keyword_search(&conn, "database", 10).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, match_memory.id);

        cleanup(path);
    }

    #[tokio::test]
    async fn keyword_search_matches_partial_cjk() {
        let (conn, path) = open_test_db("search").await;
        init_schema(&conn, 4).await.unwrap();

        let memory = create_memory(
            &conn,
            MemoryInput {
                content: "用户喜欢喝咖啡".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![0.1, 0.2, 0.3, 0.4],
        )
        .await
        .unwrap();

        // "喜欢喝茶" shares "喜", "欢", "喝" with "用户喜欢喝咖啡"
        let results = keyword_search(&conn, "喜欢喝茶", 10).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, memory.id);

        cleanup(path);
    }

    #[tokio::test]
    async fn enhanced_search_returns_empty_for_low_confidence_queries_without_keyword_hits() {
        let (conn, path) = open_test_db("search").await;
        init_schema(&conn, 4).await.unwrap();

        create_memory(
            &conn,
            MemoryInput {
                content: "database migration checklist".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![1.0, 0.0, 0.0, 0.0],
        )
        .await
        .unwrap();

        let results = enhanced_search(
            &conn,
            "completely unrelated fitness brand",
            &[0.0, 1.0, 0.0, 0.0],
            5,
            SearchOptions::default(),
        )
        .await
        .unwrap();

        assert!(results.is_empty());

        cleanup(path);
    }

    #[tokio::test]
    async fn enhanced_search_deduplicates_exact_content() {
        let (conn, path) = open_test_db("search").await;
        init_schema(&conn, 4).await.unwrap();

        for _ in 0..3 {
            create_memory(
                &conn,
                MemoryInput {
                    content: "User likes oat milk lattes".to_string(),
                    memory_type: Some(MemoryType::Semantic),
                    tags: None,
                    metadata: None,
                    importance: None,
                },
                vec![1.0, 0.0, 0.0, 0.0],
            )
            .await
            .unwrap();
        }

        create_memory(
            &conn,
            MemoryInput {
                content: "User prefers quiet hotels".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![0.9, 0.1, 0.0, 0.0],
        )
        .await
        .unwrap();

        let results = enhanced_search(
            &conn,
            "what does the user like",
            &[1.0, 0.0, 0.0, 0.0],
            5,
            SearchOptions::default(),
        )
        .await
        .unwrap();

        let unique_contents = results
            .iter()
            .map(|memory| memory.content.clone())
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(results.len(), unique_contents.len());

        cleanup(path);
    }

    #[tokio::test]
    async fn enhanced_search_limits_same_tag_signature() {
        let (conn, path) = open_test_db("search").await;
        init_schema(&conn, 4).await.unwrap();

        for content in [
            "check schema before deployment",
            "verify flags before deployment",
            "prepare rollback scripts before deployment",
        ] {
            create_memory(
                &conn,
                MemoryInput {
                    content: content.to_string(),
                    memory_type: Some(MemoryType::Procedural),
                    tags: Some(vec!["release".to_string(), "operations".to_string()]),
                    metadata: None,
                    importance: None,
                },
                vec![1.0, 0.0, 0.0, 0.0],
            )
            .await
            .unwrap();
        }

        create_memory(
            &conn,
            MemoryInput {
                content: "Do not promise zero false positives in client demos".to_string(),
                memory_type: Some(MemoryType::Procedural),
                tags: Some(vec!["demo".to_string(), "messaging".to_string()]),
                metadata: None,
                importance: None,
            },
            vec![0.95, 0.05, 0.0, 0.0],
        )
        .await
        .unwrap();

        let results = enhanced_search(
            &conn,
            "what should we watch before release and demos",
            &[1.0, 0.0, 0.0, 0.0],
            5,
            SearchOptions::default(),
        )
        .await
        .unwrap();

        let release_count = results
            .iter()
            .filter(|memory| memory.tags == vec!["release".to_string(), "operations".to_string()])
            .count();

        assert!(release_count <= MAX_RESULTS_PER_TAG_SIGNATURE);

        cleanup(path);
    }

    #[tokio::test]
    async fn explain_query_plan_executes_for_vector_and_keyword_queries() {
        let (conn, path) = open_test_db("search").await;
        init_schema(&conn, 4).await.unwrap();

        let vector_stmt = conn
            .prepare(
                r#"
                EXPLAIN QUERY PLAN
                SELECT id
                FROM memories
                WHERE embedding IS NOT NULL
                  AND vector_distance_cos(embedding, vector32(?)) < ?
                ORDER BY vector_distance_cos(embedding, vector32(?)) ASC
                LIMIT ?
                "#,
            )
            .await
            .unwrap();
        let mut vector_rows = vector_stmt
            .query(params![
                "[1.0,0.0,0.0,0.0]",
                1.0_f64,
                "[1.0,0.0,0.0,0.0]",
                5_i64
            ])
            .await
            .unwrap();
        assert!(vector_rows.next().await.unwrap().is_some());

        let keyword_stmt = conn
            .prepare(
                r#"
                EXPLAIN QUERY PLAN
                SELECT m.id
                FROM memories_fts f
                JOIN memories m ON m.rowid = f.rowid
                WHERE memories_fts MATCH ?
                ORDER BY rank
                LIMIT ?
                "#,
            )
            .await
            .unwrap();
        let mut keyword_rows = keyword_stmt
            .query(params!["database", 5_i64])
            .await
            .unwrap();
        assert!(keyword_rows.next().await.unwrap().is_some());

        cleanup(path);
    }

    #[tokio::test]
    #[ignore = "manual benchmark"]
    async fn benchmark_search_paths() {
        for size in [1_000_usize, 10_000, 50_000] {
            let (conn, path) = open_test_db("search").await;
            init_schema(&conn, 4).await.unwrap();

            for index in 0..size {
                let (content, embedding) = if index % 10 == 0 {
                    ("database performance tuning", vec![1.0, 0.0, 0.0, 0.0])
                } else {
                    ("rust ownership basics", vec![0.6, 0.4, 0.0, 0.0])
                };

                create_memory(
                    &conn,
                    MemoryInput {
                        content: format!("{content} #{index}"),
                        memory_type: Some(MemoryType::Semantic),
                        tags: None,
                        metadata: None,
                        importance: None,
                    },
                    embedding,
                )
                .await
                .unwrap();
            }

            let vector_started = Instant::now();
            let _ = vector_search(&conn, &[1.0, 0.0, 0.0, 0.0], 10, 0.3)
                .await
                .unwrap();
            let vector_elapsed = vector_started.elapsed();

            let keyword_started = Instant::now();
            let _ = keyword_search(&conn, "database", 10).await.unwrap();
            let keyword_elapsed = keyword_started.elapsed();

            let enhanced_started = Instant::now();
            let _ = enhanced_search(
                &conn,
                "database",
                &[1.0, 0.0, 0.0, 0.0],
                10,
                SearchOptions::default(),
            )
            .await
            .unwrap();
            let enhanced_elapsed = enhanced_started.elapsed();

            println!(
                "dataset={} vector={:?} keyword={:?} enhanced={:?}",
                size, vector_elapsed, keyword_elapsed, enhanced_elapsed
            );

            cleanup(path);
        }
    }
}
