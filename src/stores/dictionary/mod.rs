//! 字典库（Dictionary Store）
//!
//! 使用 sled 作为存储引擎，永久全量存档

pub mod contacts_total;
pub mod store;

pub use contacts_total::ContactsTotalStore;
pub use store::{
    compute_day_bounds_utc, compute_session_day, ConversationRecord, DayPagingConfig, DaySummary,
    DictionaryStats, DictionaryStore as SledDictionaryStore, WriteIntent, WriteIntentState,
};

/// 待提取任务树名称（与 BackgroundTaskQueue 共用；同一 sled DB 内，保证 add 可单事务原子写入）
pub const PENDING_EXTRACTION_TASKS_TREE: &str = "pending_extraction_tasks";

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::ShouxinError;
use crate::traits::store_traits::{ArchiveRecord, ContactRecord};

/// 字典库 trait
#[async_trait]
pub trait DictionaryStore: Send + Sync {
    /// 写入存档记录
    async fn write_archive(&self, record: ArchiveRecord) -> Result<(), ShouxinError>;

    /// 根据 ID 获取存档记录
    async fn get_archive(&self, archive_id: &str) -> Result<Option<ArchiveRecord>, ShouxinError>;

    /// 按时间范围查询存档记录
    async fn query_archives_by_time(
        &self,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ArchiveRecord>, ShouxinError>;

    /// 写入联系人记录
    async fn write_contact(&self, contact: ContactRecord) -> Result<(), ShouxinError>;

    /// 获取联系人记录
    async fn get_contact(&self, contact_id: &str) -> Result<Option<ContactRecord>, ShouxinError>;

    /// 获取所有联系人
    async fn get_all_contacts(
        &self,
        user_id: Option<&str>,
    ) -> Result<Vec<ContactRecord>, ShouxinError>;
}
