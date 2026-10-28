//! 消息数据结构
//!
//! 表示对话中的一条消息，用于 LLM 交互和记忆提取

use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::utils;

/// 消息角色
///
/// 定义消息的发送者类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageRole {
    /// 用户消息
    User,
    /// 助手消息
    Assistant,
    /// 系统消息
    System,
}

impl MessageRole {
    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
        }
    }
}

impl FromStr for MessageRole {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "user" => Ok(MessageRole::User),
            "assistant" => Ok(MessageRole::Assistant),
            "system" => Ok(MessageRole::System),
            _ => Ok(MessageRole::User),
        }
    }
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 消息结构体
///
/// 表示对话中的一条消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// 消息角色
    ///
    /// 标识消息的发送者：用户、助手或系统
    pub role: MessageRole,

    /// 消息内容
    ///
    /// 消息的文本内容
    pub content: String,

    /// 可选的消息名称
    ///
    /// 用于多轮对话中标识特定的参与者
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Message {
    /// 创建新的消息
    ///
    /// # 参数
    /// - `role`: 消息角色
    /// - `content`: 消息内容，不能为空
    ///
    /// # 返回
    /// 新的 Message 实例
    ///
    /// # 错误
    /// 如果内容为空，返回错误
    pub fn new(role: MessageRole, content: impl Into<String>) -> anyhow::Result<Self> {
        let content_str: String = content.into();
        utils::validate_non_empty(&content_str, "Message content")?;

        Ok(Self {
            role,
            content: content_str,
            name: None,
        })
    }

    /// 创建用户消息
    ///
    /// # 错误
    /// 如果内容为空，返回错误
    pub fn user(content: impl Into<String>) -> anyhow::Result<Self> {
        Self::new(MessageRole::User, content)
    }

    /// 创建助手消息
    ///
    /// # 错误
    /// 如果内容为空，返回错误
    pub fn assistant(content: impl Into<String>) -> anyhow::Result<Self> {
        Self::new(MessageRole::Assistant, content)
    }

    /// 创建系统消息
    ///
    /// # 错误
    /// 如果内容为空，返回错误
    pub fn system(content: impl Into<String>) -> anyhow::Result<Self> {
        Self::new(MessageRole::System, content)
    }

    /// 设置消息名称
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// 对话结构体
///
/// 表示一组连续的消息，构成一个完整的对话
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Conversation {
    /// 消息列表
    pub messages: Vec<Message>,

    /// 对话元数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Conversation {
    /// 创建新的对话
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            metadata: None,
        }
    }

    /// 添加消息
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// 添加用户消息
    ///
    /// # 错误
    /// 如果内容为空，返回错误
    pub fn add_user(&mut self, content: impl Into<String>) -> anyhow::Result<()> {
        self.messages.push(Message::user(content)?);
        Ok(())
    }

    /// 添加助手消息
    ///
    /// # 错误
    /// 如果内容为空，返回错误
    pub fn add_assistant(&mut self, content: impl Into<String>) -> anyhow::Result<()> {
        self.messages.push(Message::assistant(content)?);
        Ok(())
    }

    /// 转换为字符串格式
    ///
    /// 将所有消息合并为一个字符串，用于 LLM 输入
    pub fn to_string_format(&self) -> String {
        self.messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 获取消息数量
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// 检查对话是否为空
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}
