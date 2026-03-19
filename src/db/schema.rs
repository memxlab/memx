use crate::db::fts::preprocess_for_fts;
use crate::error::Result;
use libsql::{params, Connection};

pub async fn init_schema(conn: &Connection, embedding_dimension: usize) -> Result<()> {
    // Create the memories table
    let create_memories_sql = format!(
        r#"
        CREATE TABLE IF NOT EXISTS memories (
            id TEXT PRIMARY KEY,
            content TEXT NOT NULL,
            embedding F32_BLOB({embedding_dimension}),
            type TEXT NOT NULL,
            tags TEXT,
            metadata TEXT,
            importance REAL DEFAULT 0.5,
            access_count INTEGER DEFAULT 0,
            last_accessed_at INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#
    );
    conn.execute(&create_memories_sql, ()).await?;

    ensure_embedding_dimension(conn, embedding_dimension).await?;
    ensure_column(conn, "memories", "retrieval_count", "INTEGER DEFAULT 0").await?;
    ensure_column(conn, "memories", "last_retrieved_at", "INTEGER").await?;
    ensure_column(conn, "memories", "search_content", "TEXT").await?;

    // Backfill search_content for any existing rows that predate this column.
    // preprocess_for_fts() spaces-out CJK characters so unicode61 indexes each
    // character as an individual token, enabling partial CJK matching.
    backfill_search_content(conn).await?;

    // Drop the old B-tree vector index because it cannot accelerate vector search
    conn.execute("DROP INDEX IF EXISTS idx_embedding", ())
        .await
        .ok();

    // Try to create the DiskANN vector index (ANN approximate nearest neighbor, O(log n) search)
    // Low-dimensional vectors such as 4D test vectors may not support it; skip silently and fall back to brute-force search
    match conn
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_vec_memories ON memories(libsql_vector_idx(embedding))",
            (),
        )
        .await
    {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(
                "DiskANN vector index creation skipped (falling back to brute-force search): {}",
                e
            );
        }
    }

    // Create the remaining indexes
    conn.execute("CREATE INDEX IF NOT EXISTS idx_type ON memories(type)", ())
        .await?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_created_at ON memories(created_at DESC)",
        (),
    )
    .await?;

    // Migrate: drop the old FTS5 table that indexed raw `content`.
    // We now index `search_content` (preprocessed for CJK partial matching).
    migrate_fts(conn).await?;

    // FTS5 index on search_content (external content table, no data duplication).
    // search_content stores CJK-char-spaced text so every character is its own token.
    conn.execute(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            search_content,
            content='memories',
            content_rowid='rowid',
            tokenize='unicode61 remove_diacritics 0'
        )
        "#,
        (),
    )
    .await?;

    // Synchronization triggers: insert / delete / update
    conn.execute(
        r#"
        CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
            INSERT INTO memories_fts(rowid, search_content) VALUES (new.rowid, new.search_content);
        END
        "#,
        (),
    )
    .await?;

    conn.execute(
        r#"
        CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, search_content) VALUES('delete', old.rowid, old.search_content);
        END
        "#,
        (),
    )
    .await?;

    conn.execute(
        r#"
        CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, search_content) VALUES('delete', old.rowid, old.search_content);
            INSERT INTO memories_fts(rowid, search_content) VALUES (new.rowid, new.search_content);
        END
        "#,
        (),
    )
    .await?;

    // Rebuild the FTS index (idempotent, ensures existing data is indexed)
    conn.execute(
        "INSERT INTO memories_fts(memories_fts) VALUES('rebuild')",
        (),
    )
    .await?;

    // Create the memory_links table
    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS memory_links (
            id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL,
            target_id TEXT NOT NULL,
            link_type TEXT NOT NULL,
            strength REAL DEFAULT 0.7,
            bidirectional INTEGER DEFAULT 1,
            metadata TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (source_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY (target_id) REFERENCES memories(id) ON DELETE CASCADE
        )
        "#,
        (),
    )
    .await?;

    // Create indexes for links
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_source_id ON memory_links(source_id)",
        (),
    )
    .await?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_target_id ON memory_links(target_id)",
        (),
    )
    .await?;

    Ok(())
}

async fn ensure_embedding_dimension(conn: &Connection, expected_dimension: usize) -> Result<()> {
    let actual_dimension = embedding_dimension(conn, "memories")
        .await?
        .ok_or_else(|| {
            crate::error::AppError::Internal("memories.embedding column is missing".to_string())
        })?;

    if actual_dimension != expected_dimension {
        return Err(crate::error::AppError::Internal(format!(
            "Embedding dimension mismatch: database schema uses {}, but EMBEDDING_DIMENSION is {}. Use a database with matching vectors or run an offline migration.",
            actual_dimension, expected_dimension
        )));
    }

    Ok(())
}

async fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    if has_column(conn, table, column).await? {
        return Ok(());
    }

    let alter_sql = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    conn.execute(&alter_sql, ()).await?;
    Ok(())
}

async fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let pragma = format!("PRAGMA table_info({table})");
    let stmt = conn.prepare(&pragma).await?;
    let mut rows = stmt.query(()).await?;

    while let Some(row) = rows.next().await? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn embedding_dimension(conn: &Connection, table: &str) -> Result<Option<usize>> {
    let pragma = format!("PRAGMA table_info({table})");
    let stmt = conn.prepare(&pragma).await?;
    let mut rows = stmt.query(()).await?;

    while let Some(row) = rows.next().await? {
        let name: String = row.get(1)?;
        if name != "embedding" {
            continue;
        }

        let type_name: String = row.get(2)?;
        let dimension = parse_vector_dimension(&type_name).ok_or_else(|| {
            crate::error::AppError::Internal(format!(
                "Unsupported embedding column type for {table}.embedding: {type_name}"
            ))
        })?;
        return Ok(Some(dimension));
    }

    Ok(None)
}

/// Drop old FTS artifacts that indexed raw `content` and recreate for `search_content`.
/// Safe to call repeatedly (checks existence before acting).
async fn migrate_fts(conn: &Connection) -> Result<()> {
    // Check whether the current FTS table indexes `content` (old) or `search_content` (new).
    // If it indexes `content`, drop it and let init_schema recreate it on `search_content`.
    let stmt = conn
        .prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='memories_fts'")
        .await?;
    let mut rows = stmt.query(()).await?;

    if let Some(row) = rows.next().await? {
        let sql: String = row.get(0)?;
        if !sql.contains("search_content") {
            // Old table: drop triggers + table so they get recreated below
            for ddl in [
                "DROP TRIGGER IF EXISTS memories_ai",
                "DROP TRIGGER IF EXISTS memories_ad",
                "DROP TRIGGER IF EXISTS memories_au",
                "DROP TABLE IF EXISTS memories_fts",
            ] {
                conn.execute(ddl, ()).await.ok();
            }
        }
    }

    Ok(())
}

/// Populate search_content for any rows where it is still NULL.
/// Called once during init_schema after adding the column.
async fn backfill_search_content(conn: &Connection) -> Result<()> {
    let stmt = conn
        .prepare("SELECT rowid, content FROM memories WHERE search_content IS NULL")
        .await?;
    let mut rows = stmt.query(()).await?;

    let mut updates: Vec<(i64, String)> = Vec::new();
    while let Some(row) = rows.next().await? {
        let rowid: i64 = row.get(0)?;
        let content: String = row.get(1)?;
        updates.push((rowid, preprocess_for_fts(&content)));
    }

    for (rowid, preprocessed) in updates {
        conn.execute(
            "UPDATE memories SET search_content = ? WHERE rowid = ?",
            params![preprocessed, rowid],
        )
        .await?;
    }

    Ok(())
}

fn parse_vector_dimension(type_name: &str) -> Option<usize> {
    let normalized = type_name.trim().to_ascii_uppercase();
    // Support both F32_BLOB(n) (new) and VECTOR(n) (legacy)
    let inner = normalized
        .strip_prefix("F32_BLOB(")
        .or_else(|| normalized.strip_prefix("VECTOR("))?
        .strip_suffix(')')?;
    inner.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::init_schema;
    use crate::db::test_utils::{cleanup, open_test_db};

    #[tokio::test]
    async fn init_schema_adds_retrieval_columns_to_existing_table() {
        let (conn, path) = open_test_db("schema").await;
        conn.execute(
            r#"
            CREATE TABLE memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                embedding VECTOR(4),
                type TEXT NOT NULL,
                tags TEXT,
                metadata TEXT,
                importance REAL DEFAULT 0.5,
                access_count INTEGER DEFAULT 0,
                last_accessed_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            (),
        )
        .await
        .unwrap();

        init_schema(&conn, 4).await.unwrap();

        let stmt = conn.prepare("PRAGMA table_info(memories)").await.unwrap();
        let mut rows = stmt.query(()).await.unwrap();
        let mut has_retrieval_count = false;
        let mut has_last_retrieved_at = false;

        while let Some(row) = rows.next().await.unwrap() {
            let name: String = row.get(1).unwrap();
            if name == "retrieval_count" {
                has_retrieval_count = true;
            }
            if name == "last_retrieved_at" {
                has_last_retrieved_at = true;
            }
        }

        assert!(has_retrieval_count);
        assert!(has_last_retrieved_at);

        cleanup(path);
    }

    #[tokio::test]
    async fn init_schema_fails_when_embedding_dimension_mismatches() {
        let (conn, path) = open_test_db("schema").await;
        conn.execute(
            r#"
            CREATE TABLE memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                embedding VECTOR(8),
                type TEXT NOT NULL,
                tags TEXT,
                metadata TEXT,
                importance REAL DEFAULT 0.5,
                access_count INTEGER DEFAULT 0,
                last_accessed_at INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            (),
        )
        .await
        .unwrap();

        let err = init_schema(&conn, 4).await.unwrap_err().to_string();

        assert!(err.contains("Embedding dimension mismatch"));
        assert!(err.contains("database schema uses 8"));
        assert!(err.contains("EMBEDDING_DIMENSION is 4"));

        cleanup(path);
    }
}
