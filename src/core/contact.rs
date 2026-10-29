//! 联系人和事件数据结构
//!
//! 定义三库架构中的核心数据模型

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;

/// 所有者类型枚举
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum OwnerType {
    User,       // 用户本人
    Contact,    // 外部联系人
    Agent,      // 智能体自身
    Public,     // 公共信息
}

impl OwnerType {
    /// 稳定的字符串表示（用于 JSON metadata / SQL 字段）
    pub fn as_str(&self) -> &'static str {
        match self {
            OwnerType::User => "User",
            OwnerType::Contact => "Contact",
            OwnerType::Agent => "Agent",
            OwnerType::Public => "Public",
        }
    }

    /// 从字符串解析（大小写不敏感，兼容 "user"/"User"）
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "user" => Some(OwnerType::User),
            "contact" => Some(OwnerType::Contact),
            "agent" => Some(OwnerType::Agent),
            "public" => Some(OwnerType::Public),
            _ => None,
        }
    }
}

/// 操作范围枚举
/// 
/// 用于指定API操作是针对用户数据还是智能体数据
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Default)]
pub enum Scope {
    #[default]
    User,       // 操作用户数据
    Agent,      // 操作智能体数据
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "user" => Ok(Scope::User),
            "agent" => Ok(Scope::Agent),
            _ => Err(serde::de::Error::custom(format!(
                "invalid scope '{}': expected 'user' or 'agent'", s
            ))),
        }
    }
}

impl Scope {
    /// 将Scope转换为对应的OwnerType
    pub fn to_owner_type(&self) -> OwnerType {
        match self {
            Scope::User => OwnerType::User,
            Scope::Agent => OwnerType::Agent,
        }
    }
    
    /// 将Scope转换为所有者参数元组 (OwnerType, owner_id)
    /// 
    /// # 参数
    /// - `user_id`: 当 scope=User 时，使用此参数作为 owner_id；为 None 时使用默认值
    pub fn to_owner_params(&self, user_id: Option<&str>) -> (OwnerType, String) {
        match self {
            Scope::User => (
                OwnerType::User,
                user_id
                    .unwrap_or(crate::core::owner::DEFAULT_USER_ID)
                    .to_string(),
            ),
            Scope::Agent => (
                OwnerType::Agent,
                crate::core::owner::primary_agent_id().to_string(),
            ),
        }
    }
}

/// 联系人总表（字典库）
///
/// 存储联系人的完整信息，永久保存
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactTotal {
    /// 联系人唯一标识
    pub id: String,
    /// 联系人姓名
    ///
    /// 注意：姓名**不唯一**。两个"刘洋"是两条记录、两个独立 id。
    pub name: String,
    /// 同名消歧标签（"前端" / "后端" / "客户那边的"）
    ///
    /// 只有 `name` + `disambiguation` 都相同，才当作同一个人。
    /// 为空表示"没有额外信息可区分"，此时按姓名归并（宁可合并也不乱建号）。
    #[serde(default)]
    pub disambiguation: String,
    /// 联系人别名列表（JSON 数组）
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 是否为用户自身
    #[serde(default)]
    pub is_self: bool,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后交互时间
    pub last_interaction_at: DateTime<Utc>,
    /// 关联联系人 ID 列表（JSON 数组）
    #[serde(default)]
    pub related_contact_ids: Vec<String>,
    /// 关系标签（JSON 数组）
    #[serde(default)]
    pub relationship_tags: Vec<String>,
    /// 关系权重
    #[serde(default)]
    pub relation_weight: Option<String>,
    /// 背景信息
    #[serde(default)]
    pub background: Option<String>,
    /// 备注
    #[serde(default)]
    pub notes: Option<String>,
    /// 扩展元数据
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// 所有者类型
    pub owner_type: OwnerType,
    /// 所有者 ID
    pub owner_id: String,
    /// 是否为智能体（主智能体自身或子智能体）
    #[serde(default)]
    pub is_agent: bool,
    /// 父智能体 ID（对于子智能体）
    #[serde(default)]
    pub parent_agent_id: Option<String>,
    /// 是否为虚拟角色（游戏NPC、历史人物、AI等）
    #[serde(default)]
    pub is_virtual: bool,
}

impl ContactTotal {
    /// 创建新的联系人总表记录
    pub fn new(id: String, name: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            disambiguation: String::new(),
            aliases: Vec::new(),
            is_self: false,
            created_at: now,
            last_interaction_at: now,
            related_contact_ids: Vec::new(),
            relationship_tags: Vec::new(),
            relation_weight: None,
            background: None,
            notes: None,
            metadata: HashMap::new(),
            owner_type: OwnerType::User,
            owner_id: "default_user".to_string(),
            is_agent: false,
            parent_agent_id: None,
            is_virtual: false,
        }
    }

    /// 创建新的联系人总表记录（带所有者信息）
    pub fn new_with_owner(
        id: String,
        name: String,
        owner_type: OwnerType,
        owner_id: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            disambiguation: String::new(),
            aliases: Vec::new(),
            is_self: false,
            created_at: now,
            last_interaction_at: now,
            related_contact_ids: Vec::new(),
            relationship_tags: Vec::new(),
            relation_weight: None,
            background: None,
            notes: None,
            metadata: HashMap::new(),
            owner_type,
            owner_id,
            is_agent: false,
            parent_agent_id: None,
            is_virtual: false,
        }
    }

    /// 添加别名
    pub fn add_alias(mut self, alias: String) -> Self {
        self.aliases.push(alias);
        self
    }

    /// 添加关系标签
    pub fn add_tag(mut self, tag: String) -> Self {
        self.relationship_tags.push(tag);
        self
    }

    /// 设置背景信息
    pub fn with_background(mut self, background: String) -> Self {
        self.background = Some(background);
        self
    }

    /// 更新最后交互时间
    pub fn update_interaction(&mut self) {
        self.last_interaction_at = Utc::now();
    }
}

/// 联系人索引（关系库）
///
/// 存储活跃联系人的索引信息，用于快速查询
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactIndex {
    /// 联系人唯一标识（UUID，全局唯一；姓名的唯一性**不成立**）
    pub id: String,
    /// 联系人姓名
    ///
    /// 允许重复：两个"刘洋"各自有独立编号，靠 `disambiguation` 区分。
    pub name: String,
    /// 同名消歧标签
    ///
    /// 写入去重时按 `(name, disambiguation)` 判定：
    /// 同名且消歧相同（或都为空）→ 认为是同一个人，复用原 id；
    /// 同名但消歧不同 → 新建独立联系人。
    #[serde(default)]
    pub disambiguation: String,
    /// 联系人别名列表（JSON 字符串）
    #[serde(default)]
    pub aliases: String,
    /// 是否为用户自身
    pub is_self: bool,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后交互时间
    pub last_interaction_at: DateTime<Utc>,
    /// 关联联系人 ID 列表（JSON 字符串）
    #[serde(default)]
    pub related_contact_ids: String,
    /// 关系标签（JSON 字符串）
    #[serde(default)]
    pub relationship_tags: String,
    /// 关系权重
    #[serde(default)]
    pub relation_weight: String,
    /// 背景信息
    #[serde(default)]
    pub background: String,
    /// 备注
    #[serde(default)]
    pub notes: String,
    /// 所有者类型
    pub owner_type: OwnerType,
    /// 所有者 ID
    pub owner_id: String,
    /// 是否为智能体（主智能体自身或子智能体）
    #[serde(default)]
    pub is_agent: bool,
    /// 父智能体 ID（对于子智能体）
    #[serde(default)]
    pub parent_agent_id: Option<String>,
    /// 是否为虚拟角色（游戏NPC、历史人物、AI等）
    #[serde(default)]
    pub is_virtual: bool,
    /// 是否已归档（热冷分离）
    #[serde(default)]
    pub is_archived: bool,
    /// 归档时间
    #[serde(default)]
    pub archived_at: Option<DateTime<Utc>>,
    /// 活跃度分数（基于交互频率、事件数量等）
    #[serde(default)]
    pub activity_score: f32,
}

impl ContactIndex {
    /// 从 ContactTotal 创建 ContactIndex
    pub fn from_total(total: &ContactTotal) -> Self {
        Self {
            id: total.id.clone(),
            name: total.name.clone(),
            disambiguation: total.disambiguation.clone(),
            aliases: serde_json::to_string(&total.aliases).unwrap_or_default(),
            is_self: total.is_self,
            created_at: total.created_at,
            last_interaction_at: total.last_interaction_at,
            related_contact_ids: serde_json::to_string(&total.related_contact_ids)
                .unwrap_or_default(),
            relationship_tags: serde_json::to_string(&total.relationship_tags).unwrap_or_default(),
            relation_weight: total.relation_weight.clone().unwrap_or_default(),
            background: total.background.clone().unwrap_or_default(),
            notes: total.notes.clone().unwrap_or_default(),
            owner_type: total.owner_type,
            owner_id: total.owner_id.clone(),
            is_agent: total.is_agent,
            parent_agent_id: total.parent_agent_id.clone(),
            is_virtual: total.is_virtual,
            is_archived: false,
            archived_at: None,
            activity_score: 0.0,
        }
    }
}

/// 事件索引（关系库）
///
/// 存储事件的索引信息，用于快速查询
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EventIndex {
    /// 事件唯一标识
    pub event_id: String,
    /// 关联联系人 ID 列表（JSON 字符串）
    #[serde(default)]
    pub contact_ids: String,
    /// 事件所有者 ID
    pub owner_id: String,
    /// 事件所有者类型
    pub owner_type: OwnerType,
    /// 时间范围开始
    pub time_range_start: DateTime<Utc>,
    /// 时间范围结束
    pub time_range_end: DateTime<Utc>,
    /// 重要性（low/medium/high）
    #[serde(default)]
    pub importance: String,
    /// 状态（pending/ongoing/completed/cancelled）
    #[serde(default)]
    pub status: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 完成时间
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    /// 事件摘要
    #[serde(default)]
    pub summary: String,
    /// 事件标题
    #[serde(default)]
    pub title: Option<String>,
    /// 事件类型
    #[serde(default)]
    pub event_type: String,
    /// 截止时间
    #[serde(default)]
    pub deadline: Option<DateTime<Utc>>,
    /// 自动失效时间
    #[serde(default)]
    pub auto_invalid_at: Option<DateTime<Utc>>,
    /// 是否自动失效
    #[serde(default)]
    pub is_auto_invalid: bool,
    /// 是否已合并到其他事件
    #[serde(default)]
    pub is_merged: bool,
    /// 合并到的目标事件 ID
    #[serde(default)]
    pub merge_target_id: Option<String>,
    /// 最近一次合并检查时间
    #[serde(default)]
    pub last_merge_check_at: Option<DateTime<Utc>>,
    /// 事件实际发生时间（自然语言解析得到）
    #[serde(default)]
    pub event_time: Option<DateTime<Utc>>,
    /// LLM 评分的事件重要性（0-1）
    #[serde(default)]
    pub importance_score: f32,
    /// 是否已归档（热冷分离）
    #[serde(default)]
    pub is_archived: bool,
    /// 归档时间
    #[serde(default)]
    pub archived_at: Option<DateTime<Utc>>,
}

impl EventIndex {
    /// 创建新的事件索引
    pub fn new(event_id: String, contact_ids: Vec<String>, summary: String) -> Self {
        let now = Utc::now();
        let mut event = Self {
            event_id,
            contact_ids: serde_json::to_string(&contact_ids).unwrap_or_default(),
            owner_id: "__self__".to_string(),
            owner_type: OwnerType::User,
            time_range_start: now,
            time_range_end: now,
            importance: "medium".to_string(),
            status: "pending".to_string(),
            created_at: now,
            completed_at: None,
            summary,
            title: None,
            event_type: String::new(),
            deadline: None,
            auto_invalid_at: None,
            is_auto_invalid: false,
            is_merged: false,
            merge_target_id: None,
            last_merge_check_at: None,
            event_time: None,
            importance_score: 0.5,
            is_archived: false,
            archived_at: None,
        };
        event.compute_auto_invalid_at();
        event
    }

    /// 设置事件所有者
    pub fn with_owner(mut self, owner_id: String, owner_type: OwnerType) -> Self {
        self.owner_id = owner_id;
        self.owner_type = owner_type;
        self
    }

    /// 设置重要性
    pub fn with_importance(mut self, importance: String) -> Self {
        self.importance = importance;
        self
    }

    /// 设置状态
    pub fn with_status(mut self, status: String) -> Self {
        self.status = status;
        self
    }

    /// 完成事件
    pub fn complete(&mut self) {
        self.status = "completed".to_string();
        self.completed_at = Some(Utc::now());
    }

    /// 根据 importance_score 计算 auto_invalid_at
    /// - critical (>0.95): 永不失效 (None)
    /// - long_term (>0.7): 30天后失效
    /// - short_term (0.4-0.7): 7天后失效
    /// - low (<=0.4): 3天后失效
    pub fn compute_auto_invalid_at(&mut self) {
        self.auto_invalid_at = if self.importance_score > 0.95 {
            None // critical 永不失效
        } else if self.importance_score > 0.7 {
            Some(Utc::now() + chrono::Duration::days(30))
        } else if self.importance_score > 0.4 {
            Some(Utc::now() + chrono::Duration::days(7))
        } else {
            Some(Utc::now() + chrono::Duration::days(3))
        };
    }

    /// 设置 importance_score 并自动计算 auto_invalid_at
    pub fn with_importance_score(mut self, score: f32) -> Self {
        self.importance_score = score;
        self.compute_auto_invalid_at();
        self
    }
}

// ---------------------------------------------------------------------------
// 联系人增量更新补丁
// ---------------------------------------------------------------------------

/// 联系人增量更新补丁
///
/// 用于"只掌握联系人部分字段"的写入场景（典型：异步提取 worker 只拿到了
/// 姓名与别名）。与整条覆盖不同，`ContactPatch` 只覆盖显式提供的字段，
/// 未提供的字段保留字典库中的权威值，避免用信息量更少的视图污染源存档。
///
/// 设计约束：
/// - `contact_id` 必填，是唯一写入锚点；
/// - `name` 只在提取到更准确的名字时才覆盖；
/// - 别名是"并入"语义（去重、忽略大小写），不是替换；
/// - `owner_type` / `owner_id` 必须由调用方显式传入，禁止在存储层猜默认值。
#[derive(Debug, Clone, Default)]
pub struct ContactPatch {
    /// 联系人唯一标识
    pub contact_id: String,
    /// 姓名（None 表示不修改）
    pub name: Option<String>,
    /// 同名消歧标签（None 表示不修改）
    ///
    /// 只在"第一次拿到区分信息"时写一次："刘洋" → "刘洋[前端]"。
    /// 之后同一个人再被提到时，提取层没有标签（空字符串），不能把已有标签清掉。
    pub disambiguation: Option<String>,
    /// 最后交互时间（None 表示不修改）
    pub last_interaction_at: Option<DateTime<Utc>>,
    /// 所有者类型（None 表示保留原值）
    pub owner_type: Option<OwnerType>,
    /// 所有者 ID（None 表示保留原值）
    pub owner_id: Option<String>,
    /// 需要并入的别名（去重后追加，不覆盖已有别名）
    pub merge_aliases: Vec<String>,
    /// 需要并入的元数据（按 key 覆盖，不删除已有 key）
    pub merge_metadata: HashMap<String, serde_json::Value>,
    /// 是否智能体（None 表示不修改）
    pub is_agent: Option<bool>,
    /// 父智能体 ID（None 表示不修改）
    pub parent_agent_id: Option<String>,
    /// 是否虚拟角色（None 表示不修改）
    pub is_virtual: Option<bool>,
}

impl ContactPatch {
    /// 创建一个以 `contact_id` 为锚点的空补丁
    pub fn new(contact_id: impl Into<String>) -> Self {
        Self {
            contact_id: contact_id.into(),
            ..Default::default()
        }
    }

    /// 设置同名消歧标签（空字符串视为"不修改"）
    pub fn with_disambiguation(mut self, disambiguation: impl Into<String>) -> Self {
        let d = disambiguation.into();
        if !d.trim().is_empty() {
            self.disambiguation = Some(d.trim().to_string());
        }
        self
    }

    /// 设置姓名（会在提取到更准确名字时覆盖）
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// 设置所有者
    pub fn with_owner(mut self, owner_type: OwnerType, owner_id: impl Into<String>) -> Self {
        self.owner_type = Some(owner_type);
        self.owner_id = Some(owner_id.into());
        self
    }

    /// 更新最后交互时间
    pub fn touched_at(mut self, at: DateTime<Utc>) -> Self {
        self.last_interaction_at = Some(at);
        self
    }

    /// 并入一个别名
    pub fn merge_alias(mut self, alias: impl Into<String>) -> Self {
        self.merge_aliases.push(alias.into());
        self
    }

    /// 并入一批别名
    pub fn merge_aliases<I, S>(mut self, aliases: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.merge_aliases
            .extend(aliases.into_iter().map(Into::into));
        self
    }

    /// 并入一项元数据
    pub fn merge_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.merge_metadata.insert(key.into(), value);
        self
    }

    /// 把补丁应用到已有联系人上（只改显式提供的字段）
    pub fn apply_to(&self, contact: &mut ContactTotal) {
        if let Some(ref name) = self.name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                contact.name = trimmed.to_string();
            }
        }
        if let Some(ref d) = self.disambiguation {
            let trimmed = d.trim();
            if !trimmed.is_empty() {
                contact.disambiguation = trimmed.to_string();
            }
        }
        if let Some(at) = self.last_interaction_at {
            contact.last_interaction_at = at;
        }
        if let Some(owner_type) = self.owner_type {
            contact.owner_type = owner_type;
        }
        if let Some(ref owner_id) = self.owner_id {
            if !owner_id.is_empty() {
                contact.owner_id = owner_id.clone();
            }
        }
        for alias in &self.merge_aliases {
            let trimmed = alias.trim();
            if trimmed.is_empty() {
                continue;
            }
            let exists = contact
                .aliases
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(trimmed));
            if !exists {
                contact.aliases.push(trimmed.to_string());
            }
        }
        for (key, value) in &self.merge_metadata {
            contact.metadata.insert(key.clone(), value.clone());
        }
        if let Some(is_agent) = self.is_agent {
            contact.is_agent = is_agent;
        }
        if let Some(ref parent_agent_id) = self.parent_agent_id {
            contact.parent_agent_id = Some(parent_agent_id.clone());
        }
        if let Some(is_virtual) = self.is_virtual {
            contact.is_virtual = is_virtual;
        }
    }

    /// 当字典库中不存在该联系人时，按补丁物化一条新记录
    ///
    /// `fallback_name` 在补丁未提供姓名时使用；仍为空则回退为 `contact_id`。
    pub fn materialize(&self, fallback_name: &str) -> ContactTotal {
        let name = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_name)
            .to_string();
        let name = if name.trim().is_empty() {
            self.contact_id.clone()
        } else {
            name
        };
        let mut contact = ContactTotal::new_with_owner(
            self.contact_id.clone(),
            name,
            self.owner_type.unwrap_or(OwnerType::User),
            self.owner_id.clone().unwrap_or_else(|| "default_user".to_string()),
        );
        self.apply_to(&mut contact);
        contact
    }
}
