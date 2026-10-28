//! 存储层统一 trait
//!
//! 定义三库架构的数据模型和接口

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::ShouxinError;

/// 存档记录类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArchiveType {
    /// 对话类型
    Conversation,
    /// 事实类型
    Fact,
    /// 事件类型
    Event,
}

impl ArchiveType {
    /// 获取存档类型的字符串表示
    pub fn as_str(&self) -> &str {
        match self {
            ArchiveType::Conversation => "conversation",
            ArchiveType::Fact => "fact",
            ArchiveType::Event => "event",
        }
    }
}

/// 存档记录（字典库中的完整记录）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveRecord {
    /// 记录 ID
    pub id: String,
    /// 存档类型
    pub archive_type: ArchiveType,
    /// 用户 ID
    pub user_id: Option<String>,
    /// Agent ID
    pub agent_id: Option<String>,
    /// 内容
    pub content: String,
    /// 元数据（可选扩展字段）
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 关联的记忆 ID（可选）
    pub memory_id: Option<String>,
    /// 版本号
    pub version: u32,
}

impl ArchiveRecord {
    /// 创建新的存档记录
    pub fn new(
        archive_type: ArchiveType,
        content: String,
        user_id: Option<String>,
        agent_id: Option<String>,
        memory_id: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            archive_type,
            user_id,
            agent_id,
            content,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            memory_id,
            version: 1,
        }
    }

    /// 添加元数据
    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// 联系人记录（字典库中的联系人总表）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactRecord {
    /// 联系人 ID
    pub contact_id: String,
    /// 姓名
    pub name: String,
    /// 用户 ID
    pub user_id: Option<String>,
    /// 元数据（可选扩展字段）
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
    /// 活跃度分数
    pub activity_score: f32,
}

impl ContactRecord {
    /// 创建新的联系人记录
    pub fn new(contact_id: String, name: String, user_id: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            contact_id,
            name,
            user_id,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
            activity_score: 0.0,
        }
    }

    /// 更新活跃度
    pub fn update_activity(&mut self, delta: f32) {
        self.activity_score += delta;
        self.updated_at = Utc::now();
    }
}

/// 联系人索引（关系库中的轻量索引）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactIndex {
    /// 联系人 ID
    pub contact_id: String,
    /// 姓名
    pub name: String,
    /// 用户 ID
    pub user_id: Option<String>,
    /// 活跃度分数
    pub activity_score: f32,
    /// 最后活动时间
    pub last_activity: DateTime<Utc>,
    /// 排名
    pub rank: i32,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

impl ContactIndex {
    /// 创建新的联系人索引
    pub fn new(contact_id: String, name: String, user_id: Option<String>) -> Self {
        Self {
            contact_id,
            name,
            user_id,
            activity_score: 0.0,
            last_activity: Utc::now(),
            rank: 0,
            created_at: Utc::now(),
        }
    }

    /// 从 SQLite 行创建联系人索引
    pub fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        let last_activity_str: String = row.get("last_activity");
        let created_at_str: String = row.get("created_at");

        Ok(Self {
            contact_id: row.get("contact_id"),
            name: row.get("name"),
            user_id: row.get("user_id"),
            activity_score: row.get("activity_score"),
            last_activity: DateTime::parse_from_rfc3339(&last_activity_str)
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?
                .with_timezone(&Utc),
            rank: row.get("rank"),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?
                .with_timezone(&Utc),
        })
    }
}

/// 事件索引（关系库中的轻量索引）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventIndex {
    /// 事件 ID
    pub event_id: String,
    /// 事件标题
    pub title: Option<String>,
    /// 事件类型
    pub event_type: String,
    /// 用户 ID
    pub user_id: Option<String>,
    /// Agent ID
    pub agent_id: Option<String>,
    /// 事件时间
    pub event_time: DateTime<Utc>,
    /// 关联的存档记录 ID
    pub archive_id: String,
    /// 关联的记忆 ID（可选）
    pub memory_id: Option<String>,
    /// 重要性分数（0.0-1.0）
    pub importance: f32,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

impl EventIndex {
    /// 创建新的事件索引
    pub fn new(
        event_type: String,
        user_id: Option<String>,
        agent_id: Option<String>,
        archive_id: String,
        memory_id: Option<String>,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            title: None,
            event_type,
            user_id,
            agent_id,
            event_time: Utc::now(),
            archive_id,
            memory_id,
            importance: 0.5,
            created_at: Utc::now(),
        }
    }

    /// 从 SQLite 行创建事件索引
    pub fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        let event_time_str: String = row.get("event_time");
        let created_at_str: String = row.get("created_at");

        Ok(Self {
            event_id: row.get("event_id"),
            title: row.try_get("title").ok(),
            event_type: row.get("event_type"),
            user_id: row.get("user_id"),
            agent_id: row.get("agent_id"),
            event_time: DateTime::parse_from_rfc3339(&event_time_str)
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?
                .with_timezone(&Utc),
            archive_id: row.get("archive_id"),
            memory_id: row.get("memory_id"),
            importance: row.get("importance"),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?
                .with_timezone(&Utc),
        })
    }
}

/// 清理规则类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CleanupRule {
    /// 基于时间的清理规则（天数）
    TimeBased(u32),
    /// 基于数量的清理规则（最大数量）
    CountBased(usize),
    /// 混合清理规则（天数 + 最大数量）
    Hybrid { days: u32, max_count: usize },
}

impl Default for CleanupRule {
    /// 获取默认清理规则
    fn default() -> Self {
        CleanupRule::Hybrid {
            days: 30,
            max_count: 1000,
        }
    }
}

/// 联系人详情（统一检索返回的完整联系人信息）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactDetail {
    /// 联系人 ID
    pub contact_id: String,
    /// 姓名
    pub name: String,
    /// 别名
    pub aliases: Vec<String>,
    /// 背景信息
    pub background: Option<String>,
    /// 最后交互时间
    pub last_interaction_at: DateTime<Utc>,
    /// 活跃度分数
    pub activity_score: f32,
    /// 关联联系人
    pub related_contacts: Vec<String>,
    /// 关系标签
    pub relationship_tags: Vec<String>,
}

/// 事件详情（统一检索返回的完整事件信息）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDetail {
    /// 事件 ID
    pub event_id: String,
    /// 事件标题
    pub title: Option<String>,
    /// 事件描述
    pub description: String,
    /// 时间信息
    pub time_info: Option<String>,
    /// 重要性（low/medium/high）
    pub importance: String,
    /// 状态（pending/completed）
    pub status: String,
    /// 关联联系人
    pub related_contacts: Vec<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 完成时间
    pub completed_at: Option<DateTime<Utc>>,
}

/// 事件过滤器（用于事件检索）
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// 用户 ID
    pub user_id: Option<String>,
    /// 事件类型
    pub event_type: Option<String>,
    /// 开始时间
    pub start_time: Option<DateTime<Utc>>,
    /// 结束时间
    pub end_time: Option<DateTime<Utc>>,
    /// 重要性
    pub importance: Option<String>,
    /// 状态
    pub status: Option<String>,
    /// 搜索关键词
    pub search_query: Option<String>,
    /// 限制数量
    pub limit: usize,
    /// 是否包含已归档事件（热冷分离）
    pub include_archived: bool,
    /// 按归档月份过滤（如 "2026-06"）
    pub archived_month: Option<String>,
}

impl EventFilter {
    /// 创建新的事件过滤器
    pub fn new() -> Self {
        Self {
            limit: 100,
            ..Default::default()
        }
    }

    /// 设置用户 ID
    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// 设置时间范围
    pub fn with_time_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    /// 设置重要性
    pub fn with_importance(mut self, importance: String) -> Self {
        self.importance = Some(importance);
        self
    }

    /// 设置状态
    pub fn with_status(mut self, status: String) -> Self {
        self.status = Some(status);
        self
    }

    /// 设置搜索关键词
    pub fn with_search_query(mut self, query: String) -> Self {
        self.search_query = Some(query);
        self
    }

    /// 设置限制数量
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// 设置是否包含已归档事件
    pub fn with_include_archived(mut self, include: bool) -> Self {
        self.include_archived = include;
        self
    }

    /// 设置归档月份过滤
    pub fn with_archived_month(mut self, month: Option<String>) -> Self {
        self.archived_month = month;
        self
    }
}

/// 学习素材
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningMaterial {
    /// 素材 ID
    pub id: String,
    /// 内容
    pub content: String,
    /// 质量评分（0.0-1.0）
    pub quality_score: f32,
    /// 标签
    pub tags: Vec<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

/// 对话记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    /// 对话 ID
    pub id: String,
    /// 对话内容
    pub content: String,
    /// 用户 ID
    pub user_id: Option<String>,
    /// Agent ID
    pub agent_id: Option<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 提取状态 (v0.5.1)
    pub extracted_status: String,
    /// 提取错误信息 (v0.5.1)
    pub extraction_error: Option<String>,
}

/// 字典库 trait（永久全量存档）
#[async_trait]
pub trait DictionaryStoreTrait: Send + Sync {
    /// 保存对话到字典库
    async fn save_conversation(
        &self,
        id: &str,
        content: &str,
        timestamp: DateTime<Utc>,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<(), ShouxinError>;

    /// 原子保存对话并写入待提取任务（同一 sled 事务）
    ///
    /// 用于 `add` 的强事务边界：原文与任务要么都写入，要么都不写入。
    async fn save_conversation_with_task(
        &self,
        id: &str,
        content: &str,
        timestamp: DateTime<Utc>,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
        task_key: &str,
        task_value: &[u8],
    ) -> Result<(), ShouxinError>;

    /// 按时间范围从字典库获取对话
    async fn get_conversation_by_time_range(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<Conversation>, ShouxinError>;

    /// 保存联系人到字典库
    ///
    /// 注意：该接口以 `ContactRecord`（信息量较少的视图）为入参，实现不得用它
    /// 整条覆盖已有的权威记录，否则会丢失 aliases / background / notes 等字段。
    /// 需要精确写入时请使用 [`DictionaryStoreTrait::patch_contact`]。
    async fn save_contact(&self, contact: &ContactRecord) -> Result<(), ShouxinError>;

    /// 读取联系人总表记录（完整字段，权威存档）
    async fn get_contact_total(
        &self,
        contact_id: &str,
    ) -> Result<Option<crate::core::ContactTotal>, ShouxinError>;

    /// 以增量补丁的方式原子更新联系人（读取-合并-写入在同一存储事务内完成）
    ///
    /// 返回合并后的完整记录。联系人不存在时按补丁物化一条新记录。
    async fn patch_contact(
        &self,
        patch: &crate::core::ContactPatch,
    ) -> Result<crate::core::ContactTotal, ShouxinError>;

    /// 保存联系人历史事件到字典库（权威存档，供回溯）
    async fn save_contact_event(
        &self,
        record: &crate::stores::dictionary::store::ContactEventRecord,
    ) -> Result<(), ShouxinError>;

    /// 从字典库获取联系人
    async fn get_contact(&self, contact_id: &str) -> Result<Option<ContactRecord>, ShouxinError>;

    /// 从字典库获取所有联系人
    async fn get_all_contacts(
        &self,
        user_id: Option<&str>,
    ) -> Result<Vec<ContactRecord>, ShouxinError>;

    /// 更新对话提取状态 (v0.5.1)
    async fn update_conversation_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), ShouxinError>;

    /// 通过 conversation_id 获取对话记录 (v0.5.1)
    async fn get_conversation_by_id(&self, id: &str) -> Result<Option<Conversation>, ShouxinError>;

    /// 列出提取状态为 `failed` 的对话 ID（最多 `limit` 条）
    ///
    /// 供批量重放使用：失败对话不会自动重试，需要运维/上层显式触发。
    async fn list_failed_conversation_ids(&self, limit: usize) -> Result<Vec<String>, ShouxinError>;

    /// 开始一次跨库写入（落 `prepared` 意图）
    ///
    /// 用于"提取结果落三库"的两阶段提交：先声明意图，结束后再置
    /// `committed` / `aborted`。崩溃后据此判断哪些派生数据需要补偿清理。
    async fn begin_write_intent(
        &self,
        intent: &crate::stores::dictionary::store::WriteIntent,
    ) -> Result<(), ShouxinError>;

    /// 结束一次跨库写入（`committed` / `aborted`）
    async fn finish_write_intent(
        &self,
        conversation_id: &str,
        state: crate::stores::dictionary::store::WriteIntentState,
        error: Option<&str>,
    ) -> Result<(), ShouxinError>;

    /// 记录本次写入新建的事件/向量记忆 ID（供失败或崩溃后补偿清理）
    async fn record_write_intent_resources(
        &self,
        conversation_id: &str,
        event_ids: &[String],
        memory_ids: &[String],
    ) -> Result<(), ShouxinError>;
}

/// 关系库 trait（轻量快速索引）
#[async_trait]
pub trait RelationalStoreTrait: Send + Sync {
    /// 添加联系人索引到关系库
    async fn add_contact_index(&self, contact: ContactIndex) -> Result<(), ShouxinError>;

    /// 从关系库获取联系人索引
    async fn get_contact_index(&self, contact_id: &str) -> Result<Option<ContactIndex>, ShouxinError>;

    /// 在关系库中搜索联系人
    async fn search_contacts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactIndex>, ShouxinError>;

    /// 更新关系库中联系人的活跃度
    async fn update_contact_activity(&self, contact_id: &str, delta: f32) -> Result<(), ShouxinError>;

    /// 从关系库获取活跃联系人
    async fn get_active_contacts(
        &self,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ContactIndex>, ShouxinError>;

    /// 从关系库删除联系人索引
    async fn remove_contact_index(&self, contact_id: &str) -> Result<(), ShouxinError>;

    /// 添加事件索引到关系库
    async fn add_event_index(&self, event: EventIndex) -> Result<(), ShouxinError>;

    /// 从关系库获取事件索引
    async fn get_event_index(&self, event_id: &str) -> Result<Option<EventIndex>, ShouxinError>;

    /// 在关系库中搜索事件
    async fn search_events(&self, filter: &EventFilter) -> Result<Vec<EventIndex>, ShouxinError>;

    /// 标记事件完成
    async fn mark_event_completed(&self, event_id: &str) -> Result<(), ShouxinError>;

    /// 执行清理任务
    async fn cleanup(&self) -> Result<usize, ShouxinError>;
}

// 向量库 trait（复用现有的 VectorStore）
//
// 注意：这个 trait 在 traits/vector_store.rs 中已经定义
// 这里只是声明，实际使用时引用 crate::traits::VectorStore
