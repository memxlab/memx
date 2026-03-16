use crate::db::types::{Memory, MemoryInput, MemoryType};
use crate::error::{AppError, Result};
use chrono::Utc;
use libsql::{params, params_from_iter, Connection, Value};
use uuid::Uuid;

pub async fn create_memory(
    conn: &Connection,
    input: MemoryInput,
    embedding: Vec<f32>,
) -> Result<Memory> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();
    let memory_type = input.memory_type.unwrap_or(MemoryType::Semantic);
    let tags = serde_json::to_string(&input.tags.as_ref().cloned().unwrap_or_default())
        .map_err(|e| AppError::Internal(format!("Failed to serialize tags: {}", e)))?;
    let metadata = input
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| AppError::Internal(format!("Failed to serialize metadata: {}", e)))?;
    let importance = input.importance.unwrap_or(0.5_f64).clamp(0.0, 1.0);
    let embedding_json = serde_json::to_string(&embedding)
        .map_err(|e| AppError::Internal(format!("Failed to serialize embedding: {}", e)))?;

    conn.execute(
        r#"
        INSERT INTO memories (
            id, content, embedding, type, tags, metadata,
            importance, access_count, retrieval_count, created_at, updated_at
        ) VALUES (?, ?, vector32(?), ?, ?, ?, ?, 0, 0, ?, ?)
        "#,
        params![
            id.clone(),
            input.content.clone(),
            embedding_json,
            memory_type.as_str(),
            tags.clone(),
            metadata.clone(),
            importance,
            now,
            now
        ],
    )
    .await?;

    Ok(Memory {
        id,
        content: input.content,
        memory_type,
        tags: input.tags.unwrap_or_default(),
        metadata: input.metadata,
        importance,
        access_count: 0,
        last_accessed_at: None,
        retrieval_count: 0,
        last_retrieved_at: None,
        created_at: now,
        updated_at: now,
        score: None,
        final_score: None,
    })
}

pub async fn get_memory(conn: &Connection, id: &str) -> Result<Memory> {
    let stmt = conn
        .prepare(
            r#"
            SELECT id, content, type, tags, metadata, importance,
                   access_count, last_accessed_at, retrieval_count, last_retrieved_at,
                   created_at, updated_at
            FROM memories
            WHERE id = ?
            "#,
        )
        .await?;

    let mut rows = stmt.query(params![id]).await?;

    if let Some(row) = rows.next().await? {
        parse_memory_row(&row)
    } else {
        Err(AppError::NotFound(format!("Memory {} not found", id)))
    }
}

pub async fn update_memory(
    conn: &Connection,
    id: &str,
    content: Option<String>,
    embedding: Option<Vec<f32>>,
    importance: Option<f64>,
) -> Result<Memory> {
    let now = Utc::now().timestamp();

    if let Some(content) = content {
        let embedding_json =
            if let Some(emb) = embedding {
                Some(serde_json::to_string(&emb).map_err(|e| {
                    AppError::Internal(format!("Failed to serialize embedding: {}", e))
                })?)
            } else {
                None
            };

        if let Some(emb_json) = embedding_json {
            conn.execute(
                "UPDATE memories SET content = ?, embedding = vector32(?), updated_at = ? WHERE id = ?",
                params![content.clone(), emb_json, now, id],
            )
            .await?;
        } else {
            conn.execute(
                "UPDATE memories SET content = ?, updated_at = ? WHERE id = ?",
                params![content.clone(), now, id],
            )
            .await?;
        }
    }

    if let Some(imp) = importance {
        let clamped = imp.clamp(0.0, 1.0);
        conn.execute(
            "UPDATE memories SET importance = ?, updated_at = ? WHERE id = ?",
            params![clamped, now, id],
        )
        .await?;
    }

    get_memory(conn, id).await
}

pub async fn delete_memory(conn: &Connection, id: &str) -> Result<()> {
    let result = conn
        .execute("DELETE FROM memories WHERE id = ?", params![id])
        .await?;

    if result == 0 {
        return Err(AppError::NotFound(format!("Memory {} not found", id)));
    }

    Ok(())
}

pub async fn list_memories(
    conn: &Connection,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<Memory>> {
    let limit = limit.unwrap_or(50).min(200);
    let offset = offset.unwrap_or(0);

    let stmt = conn
        .prepare(
            r#"
            SELECT id, content, type, tags, metadata, importance,
                   access_count, last_accessed_at, retrieval_count, last_retrieved_at,
                   created_at, updated_at
            FROM memories
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .await?;

    let mut rows = stmt.query(params![limit as i64, offset as i64]).await?;
    let mut memories = Vec::new();

    while let Some(row) = rows.next().await? {
        memories.push(parse_memory_row(&row)?);
    }

    Ok(memories)
}

pub async fn track_access(conn: &Connection, id: &str) -> Result<()> {
    let now = Utc::now().timestamp();
    conn.execute(
        "UPDATE memories SET access_count = access_count + 1, last_accessed_at = ? WHERE id = ?",
        params![now, id],
    )
    .await?;
    Ok(())
}

pub async fn track_retrievals(conn: &Connection, ids: &[String]) -> Result<i64> {
    let now = Utc::now().timestamp();

    if ids.is_empty() {
        return Ok(now);
    }

    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "UPDATE memories SET retrieval_count = retrieval_count + 1, last_retrieved_at = ? WHERE id IN ({placeholders})"
    );

    let mut query_params = Vec::with_capacity(ids.len() + 1);
    query_params.push(Value::Integer(now));
    query_params.extend(ids.iter().cloned().map(Value::Text));

    conn.execute(&sql, params_from_iter(query_params)).await?;

    Ok(now)
}

pub(crate) fn parse_memory_row(row: &libsql::Row) -> Result<Memory> {
    let id: String = row.get(0)?;
    let content: String = row.get(1)?;
    let type_str: String = row.get(2)?;
    let tags_str: String = row.get(3)?;
    let metadata_str: Option<String> = row.get(4)?;
    let importance: f64 = row.get(5)?;
    let access_count: i64 = row.get(6)?;
    let last_accessed_at: Option<i64> = row.get(7)?;
    let retrieval_count: i64 = row.get(8)?;
    let last_retrieved_at: Option<i64> = row.get(9)?;
    let created_at: i64 = row.get(10)?;
    let updated_at: i64 = row.get(11)?;

    let memory_type = MemoryType::parse(&type_str)
        .ok_or_else(|| AppError::Internal(format!("Invalid memory type: {}", type_str)))?;

    let tags: Vec<String> = serde_json::from_str(&tags_str)
        .map_err(|e| AppError::Internal(format!("Failed to parse tags: {}", e)))?;

    let metadata = metadata_str
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| AppError::Internal(format!("Failed to parse metadata: {}", e)))?;

    Ok(Memory {
        id,
        content,
        memory_type,
        tags,
        metadata,
        importance,
        access_count,
        last_accessed_at,
        retrieval_count,
        last_retrieved_at,
        created_at,
        updated_at,
        score: None,
        final_score: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{create_memory, get_memory, track_access, track_retrievals};
    use crate::db::test_utils::{cleanup, open_test_db};
    use crate::db::{init_schema, MemoryInput, MemoryType};

    #[tokio::test]
    async fn track_access_does_not_touch_retrieval_stats() {
        let (conn, path) = open_test_db("memory").await;
        init_schema(&conn, 4).await.unwrap();

        let memory = create_memory(
            &conn,
            MemoryInput {
                content: "User likes coffee".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![0.1, 0.2, 0.3, 0.4],
        )
        .await
        .unwrap();

        track_access(&conn, &memory.id).await.unwrap();
        let loaded = get_memory(&conn, &memory.id).await.unwrap();

        assert_eq!(loaded.access_count, 1);
        assert!(loaded.last_accessed_at.is_some());
        assert_eq!(loaded.retrieval_count, 0);
        assert_eq!(loaded.last_retrieved_at, None);

        cleanup(path);
    }

    #[tokio::test]
    async fn track_retrievals_updates_retrieval_stats() {
        let (conn, path) = open_test_db("memory").await;
        init_schema(&conn, 4).await.unwrap();

        let memory = create_memory(
            &conn,
            MemoryInput {
                content: "User is a software engineer".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![0.4, 0.3, 0.2, 0.1],
        )
        .await
        .unwrap();

        let tracked_at = track_retrievals(&conn, &[memory.id.clone()]).await.unwrap();
        let loaded = get_memory(&conn, &memory.id).await.unwrap();

        assert_eq!(loaded.retrieval_count, 1);
        assert_eq!(loaded.last_retrieved_at, Some(tracked_at));

        cleanup(path);
    }

    #[tokio::test]
    async fn track_retrievals_updates_multiple_memories_in_one_call() {
        let (conn, path) = open_test_db("memory").await;
        init_schema(&conn, 4).await.unwrap();

        let first = create_memory(
            &conn,
            MemoryInput {
                content: "first memory".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![0.1, 0.2, 0.3, 0.4],
        )
        .await
        .unwrap();

        let second = create_memory(
            &conn,
            MemoryInput {
                content: "second memory".to_string(),
                memory_type: Some(MemoryType::Semantic),
                tags: None,
                metadata: None,
                importance: None,
            },
            vec![0.4, 0.3, 0.2, 0.1],
        )
        .await
        .unwrap();

        let tracked_at = track_retrievals(&conn, &[first.id.clone(), second.id.clone()])
            .await
            .unwrap();

        let loaded_first = get_memory(&conn, &first.id).await.unwrap();
        let loaded_second = get_memory(&conn, &second.id).await.unwrap();

        assert_eq!(loaded_first.retrieval_count, 1);
        assert_eq!(loaded_first.last_retrieved_at, Some(tracked_at));
        assert_eq!(loaded_second.retrieval_count, 1);
        assert_eq!(loaded_second.last_retrieved_at, Some(tracked_at));

        cleanup(path);
    }
}
