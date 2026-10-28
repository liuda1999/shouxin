//! 事实数据结构
//!
//! 表示从对话中提取的事实信息

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::utils;

/// 事实来源类型
///
/// 标识事实的来源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FactSource {
    /// 从用户消息中提取
    UserMessage,
    /// 从助手消息中提取
    AssistantMessage,
    /// 从系统消息中提取
    SystemMessage,
    /// 从外部知识库导入
    ExternalKnowledge,
    /// 手动添加
    Manual,
}

impl FactSource {
    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            FactSource::UserMessage => "user_message",
            FactSource::AssistantMessage => "assistant_message",
            FactSource::SystemMessage => "system_message",
            FactSource::ExternalKnowledge => "external_knowledge",
            FactSource::Manual => "manual",
        }
    }
}

/// 事实结构体
///
/// 表示从对话中提取的一条事实信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// 事实唯一标识符
    ///
    /// UUID v4 格式
    pub id: String,

    /// 事实陈述
    ///
    /// 事实的具体内容描述
    pub statement: String,

    /// 置信度
    ///
    /// 事实的可信程度，范围 [0.0, 1.0]
    /// 1.0 表示完全可信，0.0 表示不可信
    pub confidence: f32,

    /// 来源消息 ID
    ///
    /// 事实来源的消息标识符
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_message_id: Option<String>,

    /// 事实来源类型
    pub source: FactSource,

    /// 创建时间
    pub created_at: DateTime<Utc>,

    /// 更新时间
    pub updated_at: DateTime<Utc>,

    /// 元数据
    ///
    /// 附加的元信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Fact {
    /// 创建新的事实
    ///
    /// # 参数
    /// - `statement`: 事实陈述，不能为空
    /// - `confidence`: 置信度，必须在 [0.0, 1.0] 范围内
    ///
    /// # 返回
    /// 新的 Fact 实例
    ///
    /// # 错误
    /// 如果陈述为空或置信度不在有效范围内，返回错误
    pub fn new(statement: impl Into<String>, confidence: f32) -> anyhow::Result<Self> {
        let statement_str: String = statement.into();
        utils::validate_non_empty(&statement_str, "Fact statement")?;
        utils::validate_range(confidence, 0.0, 1.0, "Confidence")?;

        let now = chrono::Utc::now();
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            statement: statement_str,
            confidence,
            source_message_id: None,
            source: FactSource::UserMessage,
            created_at: now,
            updated_at: now,
            metadata: None,
        })
    }

    /// 设置来源消息 ID
    pub fn with_source_message(mut self, message_id: impl Into<String>) -> Self {
        self.source_message_id = Some(message_id.into());
        self
    }

    /// 设置来源类型
    pub fn with_source(mut self, source: FactSource) -> Self {
        self.source = source;
        self
    }

    /// 设置元数据
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// 更新事实
    ///
    /// 更新事实陈述并记录更新时间
    pub fn update(&mut self, statement: impl Into<String>) {
        self.statement = statement.into();
        self.updated_at = chrono::Utc::now();
    }

    /// 检查事实是否高置信度
    ///
    /// 置信度 >= 0.8 视为高置信度
    pub fn is_high_confidence(&self) -> bool {
        self.confidence >= 0.8
    }
}

/// 事实集合
///
/// 管理一组相关的事实
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FactCollection {
    /// 事实列表
    pub facts: Vec<Fact>,
}

impl FactCollection {
    /// 创建空的事实集合
    pub fn new() -> Self {
        Self { facts: Vec::new() }
    }

    /// 添加事实
    pub fn add(&mut self, fact: Fact) {
        self.facts.push(fact);
    }

    /// 按置信度过滤
    ///
    /// 返回置信度大于等于阈值的事实
    pub fn filter_by_confidence(&self, threshold: f32) -> Vec<&Fact> {
        self.facts
            .iter()
            .filter(|f| f.confidence >= threshold)
            .collect()
    }

    /// 获取高置信度事实
    pub fn high_confidence(&self) -> Vec<&Fact> {
        self.filter_by_confidence(0.8)
    }

    /// 获取事实数量
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}
