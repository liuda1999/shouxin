//! 关系库（Relational Store）
//!
//! 使用 SQLite + sqlx 作为存储引擎，轻量快速索引

pub mod contact_background;
pub mod contact_index;
pub mod event_index;
pub mod file_store;
pub mod maintenance;
pub mod store;
pub mod todo_list;
pub mod virtual_entity;

pub use contact_background::{ContactBackground, ContactBackgroundSummarizer};
pub use contact_index::ContactIndexStore;
pub use event_index::EventIndexStore;
pub use file_store::{FileIndex, FileRelation, FileStore};
pub use maintenance::{CleanupResult, MaintenanceService, MaintenanceState};
pub use store::{PendingEventMerge, RelationalStore};
pub use todo_list::{TodoItem, TodoListStore};
pub use virtual_entity::{
    BuiltInVirtualCharacters, NewVirtualCharacter, VirtualCharacter, VirtualCharacterAlias,
    VirtualCharacterInfo, VirtualEntityStore,
};

use async_trait::async_trait;

use crate::error::ShouxinError;
use crate::traits::store_traits::{ContactIndex, EventIndex};

/// 关系库 trait
#[async_trait]
pub trait RelationalStoreTrait: Send + Sync {
    /// 添加联系人索引
    async fn add_contact_index(&self, contact: ContactIndex) -> Result<(), ShouxinError>;

    /// 更新联系人活跃度
    async fn update_contact_activity(&self, contact_id: &str, delta: f32) -> Result<(), ShouxinError>;

    /// 获取活跃联系人（前 N）
    async fn get_active_contacts(
        &self,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ContactIndex>, ShouxinError>;

    /// 删除联系人索引
    async fn remove_contact_index(&self, contact_id: &str) -> Result<(), ShouxinError>;

    /// 添加事件索引
    async fn add_event_index(&self, event: EventIndex) -> Result<(), ShouxinError>;

    /// 查询事件索引
    async fn query_events(
        &self,
        user_id: Option<&str>,
        event_type: Option<&str>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        limit: usize,
    ) -> Result<Vec<EventIndex>, ShouxinError>;

    /// 根据事件 ID 获取索引
    async fn get_event_index(&self, event_id: &str) -> Result<Option<EventIndex>, ShouxinError>;

    /// 执行清理
    async fn cleanup(&self) -> Result<usize, ShouxinError>;
}
