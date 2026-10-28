//! 时序对话档案存储
//!
//! 存储完整对话历史，只追加不删除

use chrono::{DateTime, Utc};
use sled::Db;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::error::ShouxinError;
use crate::traits::store_traits::ArchiveRecord;

/// 时序对话档案存储
#[allow(dead_code)]
pub struct ConversationStore {
    /// sled 数据库
    db: Db,
    /// 对话档案树
    archive_tree: sled::Tree,
    /// 时间索引树
    time_index: sled::Tree,
    /// 用户索引树
    user_index: sled::Tree,
    /// 内存缓存
    cache: RwLock<HashMap<String, ArchiveRecord>>,
}

impl ConversationStore {
    /// 创建或打开对话档案存储
    pub fn new(db: Db) -> Result<Self, ShouxinError> {
        let archive_tree = db.open_tree("conversation_archives")?;
        let time_index = db.open_tree("conversation_time_index")?;
        let user_index = db.open_tree("conversation_user_index")?;

        Ok(Self {
            db,
            archive_tree,
            time_index,
            user_index,
            cache: RwLock::new(HashMap::new()),
        })
    }

    /// 生成时间索引键
    fn time_key(timestamp: DateTime<Utc>, archive_id: &str) -> String {
        format!("{}:{}", timestamp.timestamp_millis(), archive_id)
    }

    /// 生成用户索引键
    fn user_key(user_id: &str, archive_id: &str) -> String {
        format!("{}:{}", user_id, archive_id)
    }
}

impl ConversationStore {
    /// 写入对话档案
    pub async fn write_conversation(&self, record: ArchiveRecord) -> Result<(), ShouxinError> {
        let record_id = record.id.clone();
        let bytes = serde_json::to_vec(&record)?;

        self.archive_tree.insert(record_id.as_bytes(), bytes)?;

        // 更新时间索引
        let time_key = Self::time_key(record.created_at, &record_id);
        self.time_index.insert(time_key.as_bytes(), &[1])?;

        // 更新用户索引
        if let Some(user_id) = record.user_id.as_ref() {
            let user_key = Self::user_key(user_id, &record_id);
            self.user_index.insert(user_key.as_bytes(), &[1])?;
        }

        // 更新缓存
        let mut cache = self.cache.write().await;
        cache.insert(record_id, record);

        Ok(())
    }

    /// 获取对话档案
    pub async fn get_conversation(
        &self,
        archive_id: &str,
    ) -> Result<Option<ArchiveRecord>, ShouxinError> {
        {
            let cache = self.cache.read().await;
            if let Some(record) = cache.get(archive_id) {
                return Ok(Some(record.clone()));
            }
        }

        if let Some(bytes) = self.archive_tree.get(archive_id.as_bytes())? {
            let record: ArchiveRecord = serde_json::from_slice(&bytes)?;

            let mut cache = self.cache.write().await;
            cache.insert(archive_id.to_string(), record.clone());

            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    /// 按时间范围查询对话
    pub async fn query_conversations(
        &self,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ArchiveRecord>, ShouxinError> {
        let mut results = Vec::new();

        let start_millis = start_time.map(|t| t.timestamp_millis()).unwrap_or(i64::MIN);
        let end_millis = end_time.map(|t| t.timestamp_millis()).unwrap_or(i64::MAX);

        for item in self.time_index.iter() {
            let (key, _) = item?;
            let key_str = String::from_utf8_lossy(&key).to_string();
            let parts: Vec<&str> = key_str.splitn(2, ':').collect();

            if parts.len() != 2 {
                continue;
            }

            let timestamp = parts[0].parse::<i64>().unwrap_or(0);
            let archive_id = parts[1];

            if timestamp < start_millis || timestamp > end_millis {
                continue;
            }

            if let Some(archive) = self.get_conversation(archive_id).await? {
                if let Some(uid) = user_id {
                    if archive.user_id.as_deref() != Some(uid) {
                        continue;
                    }
                }
                results.push(archive);

                if limit > 0 && results.len() >= limit {
                    break;
                }
            }
        }

        Ok(results)
    }
}
