use crate::error::Result;
use libsql::{Builder, Connection};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct DbPool {
    conn: Arc<Mutex<Connection>>,
}

impl DbPool {
    pub async fn new(database_path: &str) -> Result<Self> {
        let db = Builder::new_local(database_path).build().await?;
        let conn = db.connect()?;
        conn.execute("PRAGMA cache_size = -512", ()).await?;
        conn.execute("PRAGMA temp_store = MEMORY", ()).await?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn get(&self) -> tokio::sync::MutexGuard<'_, Connection> {
        self.conn.lock().await
    }
}
