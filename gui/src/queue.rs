use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueTask {
    pub id: String,
    pub name: String,
    pub direction: String,
    pub method: String,
    pub status: String,
    pub progress: String,
    pub ticket: String,
    pub destination: String,
    pub detail: String,
    pub created_at: i64,
}

pub struct QueueDb {
    path: PathBuf,
    connection: Connection,
}

impl QueueDb {
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create GUI data directory {}", parent.display()))?;
        }
        let connection = Connection::open(&path)
            .with_context(|| format!("open queue database {}", path.display()))?;
        connection.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS transfer_tasks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                direction TEXT NOT NULL,
                method TEXT NOT NULL,
                status TEXT NOT NULL,
                progress TEXT NOT NULL,
                ticket TEXT NOT NULL,
                destination TEXT NOT NULL,
                detail TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS transfer_tasks_created_at ON transfer_tasks(created_at DESC);
            PRAGMA user_version = 1;
            ",
        )?;
        Ok(Self { path, connection })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_size(&self) -> u64 {
        fs::metadata(&self.path)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    }

    pub fn tasks(&self) -> Result<Vec<QueueTask>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, direction, method, status, progress, ticket, destination, detail, created_at
             FROM transfer_tasks ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(QueueTask {
                id: row.get(0)?,
                name: row.get(1)?,
                direction: row.get(2)?,
                method: row.get(3)?,
                status: row.get(4)?,
                progress: row.get(5)?,
                ticket: row.get(6)?,
                destination: row.get(7)?,
                detail: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read queue tasks")
    }

    pub fn upsert(&self, task: &QueueTask) -> Result<()> {
        self.connection.execute(
            "INSERT INTO transfer_tasks
             (id, name, direction, method, status, progress, ticket, destination, detail, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
               name=excluded.name, direction=excluded.direction, method=excluded.method,
               status=excluded.status, progress=excluded.progress, ticket=excluded.ticket,
               destination=excluded.destination, detail=excluded.detail, updated_at=excluded.updated_at",
            params![
                task.id,
                task.name,
                task.direction,
                task.method,
                task.status,
                task.progress,
                task.ticket,
                task.destination,
                task.detail,
                task.created_at,
                unix_time(),
            ],
        )?;
        Ok(())
    }

    pub fn clear_completed(&self) -> Result<usize> {
        Ok(self.connection.execute(
            "DELETE FROM transfer_tasks WHERE status IN ('已完成', '失败', '已取消')",
            [],
        )?)
    }

    pub fn clear_all(&self) -> Result<usize> {
        Ok(self.connection.execute("DELETE FROM transfer_tasks", [])?)
    }

    pub fn compact(&self) -> Result<()> {
        self.connection.execute_batch("VACUUM")?;
        Ok(())
    }
}

pub fn gui_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            return PathBuf::from(app_data).join("ii");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Application Support/ii");
        }
    }
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("ii");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/ii");
    }
    std::env::temp_dir().join("ii")
}

pub fn unix_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_round_trip_and_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let database = QueueDb::open(directory.path().join("queue.db")).unwrap();
        let task = QueueTask {
            id: "task-1".into(),
            name: "example.txt".into(),
            direction: "发送".into(),
            method: "仅局域网".into(),
            status: "已完成".into(),
            progress: "100%".into(),
            ticket: "ii1example".into(),
            destination: "远端设备".into(),
            detail: "done".into(),
            created_at: unix_time(),
        };
        database.upsert(&task).unwrap();
        assert_eq!(database.tasks().unwrap(), vec![task]);
        assert_eq!(database.clear_completed().unwrap(), 1);
        assert!(database.tasks().unwrap().is_empty());
    }

    #[test]
    fn queue_reopens_and_preserves_original_creation_time() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("queue.db");
        let database = QueueDb::open(path.clone()).unwrap();
        let mut task = QueueTask {
            id: "task-1".into(),
            name: "example.txt".into(),
            direction: "发送".into(),
            method: "仅局域网".into(),
            status: "准备中".into(),
            progress: "等待连接".into(),
            ticket: String::new(),
            destination: "远端设备".into(),
            detail: "created".into(),
            created_at: 10,
        };
        database.upsert(&task).unwrap();
        task.status = "已完成".into();
        task.detail = "done".into();
        task.created_at = 20;
        database.upsert(&task).unwrap();
        drop(database);

        let reopened = QueueDb::open(path).unwrap();
        let tasks = reopened.tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "已完成");
        assert_eq!(tasks[0].created_at, 10);
    }
}
