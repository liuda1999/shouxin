//! LLM 客户端 Trait
//!
//! 定义与大语言模型交互的标准接口

use async_trait::async_trait;
use std::collections::HashMap;

use crate::core::{Fact, Message};
use crate::error::ShouxinError;

/// LLM 客户端 Trait
///
/// 定义与大语言模型交互的标准接口
/// 支持多种 LLM 后端（如 Ollama、OpenAI 等）
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 发送聊天请求
    ///
    /// # 参数
    /// - `messages`: 消息列表，包含对话历史
    ///
    /// # 返回
    /// LLM 生成的回复文本
    ///
    /// # 示例
    /// ```ignore
    /// let messages = vec![
    ///     Message::user("你好"),
    /// ];
    /// let response = llm.chat(messages).await?;
    /// ```
    async fn chat(&self, messages: Vec<Message>) -> Result<String, ShouxinError>;

    /// 从对话中提取事实
    ///
    /// # 参数
    /// - `conversation`: 对话文本
    ///
    /// # 返回
    /// 从对话中提取的事实列表
    ///
    /// # 说明
    /// 使用 LLM 分析对话内容，提取有价值的事实信息
    async fn extract_facts(&self, conversation: &str) -> Result<Vec<Fact>, ShouxinError>;

    /// 决定记忆操作
    ///
    /// # 参数
    /// - `new_fact`: 新提取的事实
    /// - `existing_memories`: 现有的相关记忆
    ///
    /// # 返回
    /// 应该执行的操作类型（添加、更新、删除等）
    ///
    /// # 说明
    /// 使用 LLM 判断新事实与现有记忆的关系，
    /// 决定是否需要添加、更新或删除记忆
    async fn decide_memory_action(
        &self,
        new_fact: &Fact,
        existing_memories: Vec<&crate::core::Memory>,
    ) -> Result<crate::core::MemoryAction, ShouxinError>;

    /// 获取模型名称
    ///
    /// # 返回
    /// 当前使用的模型名称
    fn model_name(&self) -> &str;

    /// 生成文本补全
    ///
    /// # 参数
    /// - `prompt`: 提示文本
    ///
    /// # 返回
    /// 补全后的文本
    async fn complete(&self, prompt: &str) -> Result<String, ShouxinError>;

    /// 预加载模型（服务启动时调用）
    ///
    /// 对于需要加载本地模型文件的后端（如 GGUF），
    /// 此方法在服务启动时预先加载模型，避免首次请求超时。
    /// 默认实现为空操作。
    async fn preload(&self) -> Result<(), ShouxinError> {
        Ok(())
    }

    /// 获取嵌入向量（如果 LLM 支持）
    ///
    /// # 参数
    /// - `text`: 输入文本
    ///
    /// # 返回
    /// 文本的嵌入向量
    ///
    /// # 说明
    /// 部分 LLM 服务同时提供嵌入功能
    async fn embed(&self, _text: &str) -> Result<Vec<f32>, ShouxinError> {
        Err(ShouxinError::NotImplemented(
            "embed not supported by this LLM client".into(),
        ))
    }
}

/// LLM 配置
///
/// LLM 客户端的配置参数
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// 模型名称
    pub model: String,

    /// API 端点
    pub endpoint: Option<String>,

    /// 温度参数
    ///
    /// 控制生成的随机性，范围 0.0-2.0
    /// 0.0 最确定，2.0 最随机
    pub temperature: f32,

    /// 最大生成令牌数
    pub max_tokens: Option<u32>,

    /// 系统提示词
    pub system_prompt: Option<String>,

    /// 额外参数
    pub extra_params: HashMap<String, serde_json::Value>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model: "llama3".to_string(),
            endpoint: None,
            temperature: 0.7,
            max_tokens: None,
            system_prompt: None,
            extra_params: HashMap::new(),
        }
    }
}

impl LlmConfig {
    /// 创建新的配置
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Default::default()
        }
    }

    /// 设置端点
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// 设置温度
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// 设置最大令牌数
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// 设置系统提示词
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }
}
