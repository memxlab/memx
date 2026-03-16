use libsql::{Builder, Connection};
use std::{fs, path::PathBuf};
use uuid::Uuid;

pub async fn open_test_db(prefix: &str) -> (Connection, PathBuf) {
    let path = std::env::temp_dir().join(format!("memx-{prefix}-test-{}.db", Uuid::new_v4()));
    let db = Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    (conn, path)
}

pub fn cleanup(path: PathBuf) {
    let _ = fs::remove_file(&path);
    let shm = PathBuf::from(format!("{}-shm", path.display()));
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let _ = fs::remove_file(shm);
    let _ = fs::remove_file(wal);
}
