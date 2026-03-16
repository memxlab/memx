use crate::db::types::{LinkInput, LinkType, MemoryLink};
use crate::error::{AppError, Result};
use chrono::Utc;
use libsql::{params, Connection};
use uuid::Uuid;

pub async fn create_link(conn: &Connection, input: LinkInput) -> Result<MemoryLink> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();
    let strength = input.strength.unwrap_or(0.7_f64).clamp(0.0, 1.0);
    let bidirectional = input.bidirectional.unwrap_or(true);
    let metadata = input
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| AppError::Internal(format!("Failed to serialize metadata: {}", e)))?;

    conn.execute(
        r#"
        INSERT INTO memory_links (
            id, source_id, target_id, link_type, strength, 
            bidirectional, metadata, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        params![
            id.clone(),
            input.source_id.clone(),
            input.target_id.clone(),
            input.link_type.as_str(),
            strength,
            if bidirectional { 1 } else { 0 },
            metadata.clone(),
            now,
            now
        ],
    )
    .await?;

    Ok(MemoryLink {
        id,
        source_id: input.source_id,
        target_id: input.target_id,
        link_type: input.link_type,
        strength,
        bidirectional,
        metadata: input.metadata,
        created_at: now,
        updated_at: now,
    })
}

pub async fn get_links(
    conn: &Connection,
    memory_id: &str,
    include_reverse: bool,
) -> Result<Vec<MemoryLink>> {
    let query = if include_reverse {
        r#"
        SELECT id, source_id, target_id, link_type, strength, 
               bidirectional, metadata, created_at, updated_at
        FROM memory_links
        WHERE source_id = ? OR (target_id = ? AND bidirectional = 1)
        ORDER BY strength DESC
        "#
    } else {
        r#"
        SELECT id, source_id, target_id, link_type, strength, 
               bidirectional, metadata, created_at, updated_at
        FROM memory_links
        WHERE source_id = ?
        ORDER BY strength DESC
        "#
    };

    let stmt = conn.prepare(query).await?;

    let mut rows = if include_reverse {
        stmt.query(params![memory_id, memory_id]).await?
    } else {
        stmt.query(params![memory_id]).await?
    };

    let mut links = Vec::new();

    while let Some(row) = rows.next().await? {
        links.push(parse_link_row(row)?);
    }

    Ok(links)
}

pub async fn delete_link(conn: &Connection, link_id: &str) -> Result<()> {
    let result = conn
        .execute("DELETE FROM memory_links WHERE id = ?", params![link_id])
        .await?;

    if result == 0 {
        return Err(AppError::NotFound(format!("Link {} not found", link_id)));
    }

    Ok(())
}

pub async fn get_link_count(conn: &Connection, memory_id: &str) -> Result<i64> {
    let stmt = conn
        .prepare(
            r#"
            SELECT COUNT(*) 
            FROM memory_links 
            WHERE source_id = ? OR (target_id = ? AND bidirectional = 1)
            "#,
        )
        .await?;

    let mut rows = stmt.query(params![memory_id, memory_id]).await?;

    if let Some(row) = rows.next().await? {
        let count: i64 = row.get(0)?;
        Ok(count)
    } else {
        Ok(0)
    }
}

fn parse_link_row(row: libsql::Row) -> Result<MemoryLink> {
    let id: String = row.get(0)?;
    let source_id: String = row.get(1)?;
    let target_id: String = row.get(2)?;
    let link_type_str: String = row.get(3)?;
    let strength: f64 = row.get(4)?;
    let bidirectional_int: i64 = row.get(5)?;
    let metadata_str: Option<String> = row.get(6)?;
    let created_at: i64 = row.get(7)?;
    let updated_at: i64 = row.get(8)?;

    let link_type = LinkType::parse(&link_type_str)
        .ok_or_else(|| AppError::Internal(format!("Invalid link type: {}", link_type_str)))?;

    let metadata = metadata_str
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| AppError::Internal(format!("Failed to parse metadata: {}", e)))?;

    Ok(MemoryLink {
        id,
        source_id,
        target_id,
        link_type,
        strength,
        bidirectional: bidirectional_int != 0,
        metadata,
        created_at,
        updated_at,
    })
}
