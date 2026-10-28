//! 记忆数据结构
//!
//! Shouxin 的核心数据结构，表示一条记忆记录

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use super::embedding::Embedding;
use crate::utils;

/// 记忆类型
///
/// 定义记忆的分类
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MemoryType {
    /// 短期记忆
    ///
    /// 临时的、易变的记忆，通常在会话结束后清除
    ShortTerm,

    /// 长期记忆
    ///
    /// 持久化的、重要的记忆，需要长期保存
    #[default]
    LongTerm,

    /// 事实记忆
    ///
    /// 客观事实信息，如用户偏好、个人资料等
    Fact,

    /// 事件记忆
    ///
    /// 特定事件的记录，如对话历史、操作记录等
    Event,

    /// 情感记忆
    ///
    /// 与情感相关的记忆，如用户情绪状态等
    Emotional,
}

impl MemoryType {
    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::ShortTerm => "short_term",
            MemoryType::LongTerm => "long_term",
            MemoryType::Fact => "fact",
            MemoryType::Event => "event",
            MemoryType::Emotional => "emotional",
        }
    }
}

impl FromStr for MemoryType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "short_term" => Ok(MemoryType::ShortTerm),
            "long_term" => Ok(MemoryType::LongTerm),
            "fact" => Ok(MemoryType::Fact),
            "event" => Ok(MemoryType::Event),
            "emotional" => Ok(MemoryType::Emotional),
            _ => Ok(MemoryType::LongTerm),
        }
    }
}

/// 记忆操作类型
///
/// 定义对记忆的操作类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryAction {
    /// 添加新记忆
    Add,
    /// 更新现有记忆
    Update,
    /// 删除记忆
    Delete,
    /// 无需操作
    Noop,
}

/// 记忆状态
///
/// 定义记忆的生命周期状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MemoryState {
    /// 活跃状态
    #[default]
    Active,
    /// 归档状态
    Archived,
    /// 待删除状态
    PendingDeletion,
}

/// 记忆结构体
///
/// Shouxin 的核心数据结构，表示一条完整的记忆记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// 记忆唯一标识符
    ///
    /// UUID v4 格式，全局唯一
    pub id: String,

    /// 用户标识符
    ///
    /// 标识记忆所属的用户
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// 代理标识符
    ///
    /// 标识记忆所属的代理/会话
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    /// 记忆内容
    ///
    /// 记忆的文本内容
    pub content: String,

    /// 嵌入向量
    ///
    /// 内容的向量表示，用于语义搜索
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Embedding>,

    /// 元数据
    ///
    /// 附加的结构化信息
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,

    /// 创建时间
    pub created_at: DateTime<Utc>,

    /// 更新时间
    pub updated_at: DateTime<Utc>,

    /// 记忆类型
    #[serde(default)]
    pub memory_type: MemoryType,

    /// 记忆状态
    #[serde(default)]
    pub state: MemoryState,

    /// 记忆哈希值
    ///
    /// 用于检测重复记忆
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,

    /// 记忆分数
    ///
    /// 记忆的重要性或相关性分数
    #[serde(default)]
    pub score: f32,
}

impl Memory {
    /// 创建新的记忆
    ///
    /// # 参数
    /// - `content`: 记忆内容，不能为空
    ///
    /// # 返回
    /// 新的 Memory 实例
    ///
    /// # 错误
    /// 如果内容为空，返回错误
    pub fn new(content: impl Into<String>) -> anyhow::Result<Self> {
        let content_str: String = content.into();
        utils::validate_non_empty(&content_str, "Memory content")?;

        let now = chrono::Utc::now();
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            user_id: None,
            agent_id: None,
            content: content_str,
            embedding: None,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
            memory_type: MemoryType::default(),
            state: MemoryState::default(),
            hash: None,
            score: 0.0,
        })
    }

    /// 设置用户 ID
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// 设置代理 ID
    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    /// 设置记忆类型
    pub fn with_type(mut self, memory_type: MemoryType) -> Self {
        self.memory_type = memory_type;
        self
    }

    /// 设置嵌入向量
    pub fn with_embedding(mut self, embedding: Embedding) -> Self {
        self.embedding = Some(embedding);
        self
    }

    /// 添加元数据
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// 更新时间戳
    pub fn touch(&mut self) {
        self.updated_at = chrono::Utc::now();
    }

    /// 计算内容哈希
    ///
    /// 用于检测重复记忆
    pub fn compute_hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.content.hash(&mut hasher);
        if let Some(ref user_id) = self.user_id {
            user_id.hash(&mut hasher);
        }
        format!("{:x}", hasher.finish())
    }

    /// 检查是否为长期记忆
    pub fn is_long_term(&self) -> bool {
        self.memory_type == MemoryType::LongTerm
    }

    /// 检查是否为活跃状态
    pub fn is_active(&self) -> bool {
        self.state == MemoryState::Active
    }

    /// 归档记忆
    pub fn archive(&mut self) {
        self.state = MemoryState::Archived;
        self.touch();
    }

    /// 标记为待删除
    pub fn mark_for_deletion(&mut self) {
        self.state = MemoryState::PendingDeletion;
        self.touch();
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new("default").expect("Failed to create default Memory")
    }
}

/// 记忆过滤器
///
/// 用于搜索和筛选记忆
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct MemoryFilter {
    /// 用户 ID 过滤
    pub user_id: Option<String>,

    /// 代理 ID 过滤
    pub agent_id: Option<String>,

    /// 记忆类型过滤
    pub memory_type: Option<MemoryType>,

    /// 记忆状态过滤
    pub state: Option<MemoryState>,

    /// 元数据过滤
    pub metadata_filters: HashMap<String, serde_json::Value>,

    /// 时间范围过滤（开始）
    pub start_time: Option<DateTime<Utc>>,

    /// 时间范围过滤（结束）
    pub end_time: Option<DateTime<Utc>>,
}

impl MemoryFilter {
    /// 创建新的过滤器
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置用户 ID 过滤
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// 设置代理 ID 过滤
    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    /// 设置记忆类型过滤
    pub fn with_type(mut self, memory_type: MemoryType) -> Self {
        self.memory_type = Some(memory_type);
        self
    }

    /// 添加元数据过滤条件
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata_filters.insert(key.into(), value);
        self
    }

    /// 检查记忆是否匹配过滤器
    pub fn matches(&self, memory: &Memory) -> bool {
        if let Some(ref user_id) = self.user_id {
            if memory.user_id.as_ref() != Some(user_id) {
                return false;
            }
        }

        if let Some(ref agent_id) = self.agent_id {
            if memory.agent_id.as_ref() != Some(agent_id) {
                return false;
            }
        }

        if let Some(ref memory_type) = self.memory_type {
            if memory.memory_type != *memory_type {
                return false;
            }
        }

        if let Some(ref state) = self.state {
            if memory.state != *state {
                return false;
            }
        }

        if let Some(ref start_time) = self.start_time {
            if memory.created_at < *start_time {
                return false;
            }
        }

        if let Some(ref end_time) = self.end_time {
            if memory.created_at > *end_time {
                return false;
            }
        }

        true
    }
}
