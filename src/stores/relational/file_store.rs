use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{query, Pool, Row, Sqlite};

use crate::error::ShouxinError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIndex {
    pub file_id: String,
    pub file_name: String,
    pub file_type: String,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub md5_hash: Option<String>,
    pub owner_type: String,
    pub owner_id: String,
    pub uploaded_at: Option<String>,
    pub processed_at: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub original_path: Option<String>,
    pub extracted_text_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRelation {
    pub file_id: String,
    pub target_type: String,
    pub target_id: String,
    pub relation_type: String,
    pub created_at: Option<String>,
}

#[derive(Clone)]
pub struct FileStore {
    pool: Pool<Sqlite>,
}

impl FileStore {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn init_schema(&self) -> Result<(), ShouxinError> {
        query(
            r#"
            CREATE TABLE IF NOT EXISTS file_index (
                file_id TEXT PRIMARY KEY,
                file_name TEXT NOT NULL,
                file_type TEXT NOT NULL,
                mime_type TEXT,
                file_size INTEGER,
                md5_hash TEXT,
                owner_type TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                uploaded_at TEXT,
                processed_at TEXT,
                status TEXT DEFAULT 'pending',
                error_message TEXT,
                original_path TEXT,
                extracted_text_summary TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_file_owner ON file_index(owner_type, owner_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_file_md5 ON file_index(md5_hash)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // V9: 同一 owner 下相同 MD5 的文件唯一约束，防止并发上传竞态
        let _ = query(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_file_md5_owner
            ON file_index(md5_hash, owner_type, owner_id)
            "#,
        )
        .execute(&self.pool)
        .await;

        query(
            r#"
            CREATE TABLE IF NOT EXISTS file_relations (
                file_id TEXT NOT NULL,
                target_type TEXT NOT NULL,
                target_id TEXT NOT NULL,
                relation_type TEXT DEFAULT 'attached',
                created_at TEXT,
                PRIMARY KEY (file_id, target_type, target_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 插入文件索引
    ///
    /// 返回 `true` 表示插入成功，`false` 表示命中唯一约束（并发去重竞态）。
    /// 调用方必须处理 `false`，否则会出现"接口返回了新 file_id、库里却没有这条记录"（F-F1）。
    pub async fn insert_file(&self, file: &FileIndex) -> Result<bool, ShouxinError> {
        let result = query(
            r#"
            INSERT OR IGNORE INTO file_index (file_id, file_name, file_type, mime_type, file_size, md5_hash,
                owner_type, owner_id, uploaded_at, processed_at, status, error_message,
                original_path, extracted_text_summary)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&file.file_id)
        .bind(&file.file_name)
        .bind(&file.file_type)
        .bind(&file.mime_type)
        .bind(file.file_size)
        .bind(&file.md5_hash)
        .bind(&file.owner_type)
        .bind(&file.owner_id)
        .bind(&file.uploaded_at)
        .bind(&file.processed_at)
        .bind(&file.status)
        .bind(&file.error_message)
        .bind(&file.original_path)
        .bind(&file.extracted_text_summary)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn get_file(&self, file_id: &str) -> Result<Option<FileIndex>, ShouxinError> {
        let row = query(
            r#"
            SELECT file_id, file_name, file_type, mime_type, file_size, md5_hash,
                owner_type, owner_id, uploaded_at, processed_at, status, error_message,
                original_path, extracted_text_summary
            FROM file_index WHERE file_id = ?
            "#,
        )
        .bind(file_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(FileIndex {
                file_id: r.get("file_id"),
                file_name: r.get("file_name"),
                file_type: r.get("file_type"),
                mime_type: r.get("mime_type"),
                file_size: r.get("file_size"),
                md5_hash: r.get("md5_hash"),
                owner_type: r.get("owner_type"),
                owner_id: r.get("owner_id"),
                uploaded_at: r.get("uploaded_at"),
                processed_at: r.get("processed_at"),
                status: r.get("status"),
                error_message: r.get("error_message"),
                original_path: r.get("original_path"),
                extracted_text_summary: r.get("extracted_text_summary"),
            })),
            None => Ok(None),
        }
    }

    pub async fn get_file_by_md5_and_owner(
        &self,
        md5_hash: &str,
        owner_type: &str,
        owner_id: &str,
    ) -> Result<Option<FileIndex>, ShouxinError> {
        let row = query(
            r#"
            SELECT file_id, file_name, file_type, mime_type, file_size, md5_hash,
                owner_type, owner_id, uploaded_at, processed_at, status, error_message,
                original_path, extracted_text_summary
            FROM file_index WHERE md5_hash = ? AND owner_type = ? AND owner_id = ?
            ORDER BY uploaded_at DESC LIMIT 1
            "#,
        )
        .bind(md5_hash)
        .bind(owner_type)
        .bind(owner_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(FileIndex {
                file_id: r.get("file_id"),
                file_name: r.get("file_name"),
                file_type: r.get("file_type"),
                mime_type: r.get("mime_type"),
                file_size: r.get("file_size"),
                md5_hash: r.get("md5_hash"),
                owner_type: r.get("owner_type"),
                owner_id: r.get("owner_id"),
                uploaded_at: r.get("uploaded_at"),
                processed_at: r.get("processed_at"),
                status: r.get("status"),
                error_message: r.get("error_message"),
                original_path: r.get("original_path"),
                extracted_text_summary: r.get("extracted_text_summary"),
            })),
            None => Ok(None),
        }
    }

    pub async fn update_file_status(
        &self,
        file_id: &str,
        status: &str,
        error_message: Option<&str>,
        extracted_text_summary: Option<&str>,
    ) -> Result<(), ShouxinError> {
        let now = Utc::now().to_rfc3339();
        query(
            r#"
            UPDATE file_index SET status = ?, processed_at = ?, error_message = ?,
                extracted_text_summary = ?
            WHERE file_id = ?
            "#,
        )
        .bind(status)
        .bind(if status == "completed" || status == "failed" { Some(&now) } else { None })
        .bind(error_message)
        .bind(extracted_text_summary)
        .bind(file_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_file(&self, file_id: &str) -> Result<(), ShouxinError> {
        query("DELETE FROM file_index WHERE file_id = ?")
            .bind(file_id)
            .execute(&self.pool)
            .await?;

        query("DELETE FROM file_relations WHERE file_id = ?")
            .bind(file_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn insert_file_relation(
        &self,
        file_id: &str,
        target_type: &str,
        target_id: &str,
    ) -> Result<(), ShouxinError> {
        query(
            r#"
            INSERT OR IGNORE INTO file_relations (file_id, target_type, target_id, relation_type, created_at)
            VALUES (?, ?, ?, 'attached', ?)
            "#,
        )
        .bind(file_id)
        .bind(target_type)
        .bind(target_id)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_files_by_owner(
        &self,
        owner_type: &str,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<FileIndex>, ShouxinError> {
        let rows = query(
            r#"
            SELECT file_id, file_name, file_type, mime_type, file_size, md5_hash,
                owner_type, owner_id, uploaded_at, processed_at, status, error_message,
                original_path, extracted_text_summary
            FROM file_index WHERE owner_type = ? AND owner_id = ?
            ORDER BY uploaded_at DESC LIMIT ?
            "#,
        )
        .bind(owner_type)
        .bind(owner_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let files = rows
            .iter()
            .map(|r| FileIndex {
                file_id: r.get("file_id"),
                file_name: r.get("file_name"),
                file_type: r.get("file_type"),
                mime_type: r.get("mime_type"),
                file_size: r.get("file_size"),
                md5_hash: r.get("md5_hash"),
                owner_type: r.get("owner_type"),
                owner_id: r.get("owner_id"),
                uploaded_at: r.get("uploaded_at"),
                processed_at: r.get("processed_at"),
                status: r.get("status"),
                error_message: r.get("error_message"),
                original_path: r.get("original_path"),
                extracted_text_summary: r.get("extracted_text_summary"),
            })
            .collect();

        Ok(files)
    }
}