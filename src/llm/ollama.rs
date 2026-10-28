//! Ollama LLM 客户端实现
//!
//! 通过 reqwest 直接调用本地 Ollama 服务的 HTTP API，提供聊天、事实提取和记忆决策功能

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::core::{Fact, FactSource, Memory, MemoryAction, Message, MessageRole};
use crate::error::{ShouxinError, Result};
use crate::traits::llm::{LlmClient, LlmConfig};

/// 默认 HTTP 请求超时时间（秒）
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// 最大输入文本长度（字符数）
const MAX_INPUT_LENGTH: usize = 100_000;

/// 最大消息数量
const MAX_MESSAGES: usize = 100;

/// Ollama 聊天请求
#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

/// Ollama 聊天消息
#[derive(Debug, Serialize, Deserialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
}

/// Ollama 聊天响应
#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: Option<OllamaChatMessage>,
}

/// Ollama LLM 客户端
///
/// 使用 reqwest 直接调用 Ollama HTTP API
///
/// # 配置说明
/// - base_url: Ollama 服务地址，默认 http://localhost:11434
/// - model: 使用的模型名称，如 "gpt-oss:20b"
/// - temperature: 温度参数，控制生成随机性，0.0-2.0
/// - max_tokens: 最大生成令牌数
///
/// # 安全特性
/// - 输入验证：验证模型名称、温度、max_tokens 的有效性
/// - 超时控制：默认 120 秒超时，防止无限等待
/// - 长度限制：限制输入文本长度和消息数量
/// - URL 验证：只允许 http/https 协议
///
/// # 前置条件
/// - Ollama 服务已启动: `ollama serve`
/// - 模型已拉取: `ollama pull gpt-oss:20b`
pub struct OllamaLlmClient {
    /// HTTP 客户端
    client: Client,
    /// 基础 URL
    base_url: String,
    /// 模型名称
    model: String,
    /// 温度参数
    temperature: f32,
    /// 最大生成令牌数
    max_tokens: Option<u32>,
}

impl OllamaLlmClient {
    /// 创建新的 Ollama LLM 客户端
    ///
    /// # 参数
    /// - model: 模型名称，如 "gpt-oss:20b"
    ///
    /// # 返回
    /// 新的 OllamaLlmClient 实例
    pub fn new(model: impl Into<String>) -> Self {
        Self::with_config(LlmConfig::new(model))
    }

    /// 使用配置创建 Ollama LLM 客户端
    ///
    /// # 参数
    /// - config: LLM 配置
    ///
    /// # 返回
    /// 新的 OllamaLlmClient 实例
    ///
    /// # 错误
    /// - 如果 URL 无效（非 http/https）
    /// - 如果温度参数超出范围
    pub fn with_config(config: LlmConfig) -> Self {
        let base_url = config
            .endpoint
            .unwrap_or_else(|| "http://localhost:11434".to_string());

        let temperature = config.temperature.clamp(0.0, 2.0);

        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            base_url,
            model: config.model,
            temperature,
            max_tokens: config.max_tokens,
        }
    }

    /// 创建默认的 Ollama LLM 客户端
    ///
    /// 使用默认模型 "gpt-oss:20b"
    ///
    /// # 返回
    /// 新的 OllamaLlmClient 实例
    pub fn new_default() -> Self {
        Self::new("gpt-oss:20b")
    }

    /// 验证 URL 是否有效
    ///
    /// # 参数
    /// - url: 要验证的 URL
    ///
    /// # 返回
    /// 如果 URL 有效返回 true，否则返回 false
    fn is_valid_url(url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }

    /// 验证输入文本长度
    ///
    /// # 参数
    /// - text: 要验证的文本
    ///
    /// # 返回
    /// 如果文本长度有效返回 Ok(())，否则返回错误
    fn validate_input_length(text: &str) -> Result<()> {
        if text.len() > MAX_INPUT_LENGTH {
            return Err(ShouxinError::LLMError(format!(
                "输入文本过长: {} 字符，最大允许 {} 字符",
                text.len(),
                MAX_INPUT_LENGTH
            )));
        }
        Ok(())
    }

    /// 验证消息数量
    ///
    /// # 参数
    /// - messages: 要验证的消息列表
    ///
    /// # 返回
    /// 如果消息数量有效返回 Ok(())，否则返回错误
    fn validate_message_count(messages: &[Message]) -> Result<()> {
        if messages.len() > MAX_MESSAGES {
            return Err(ShouxinError::LLMError(format!(
                "消息数量过多: {} 条，最大允许 {} 条",
                messages.len(),
                MAX_MESSAGES
            )));
        }
        Ok(())
    }

    /// 构建聊天请求
    fn build_chat_request(&self, messages: Vec<OllamaChatMessage>) -> OllamaChatRequest {
        OllamaChatRequest {
            model: self.model.clone(),
            messages,
            stream: false,
            temperature: Some(self.temperature),
            max_tokens: self.max_tokens,
        }
    }

    /// 转换 Shouxin 消息为 Ollama 消息
    fn convert_messages(&self, messages: Vec<Message>) -> Vec<OllamaChatMessage> {
        messages
            .into_iter()
            .map(|msg| OllamaChatMessage {
                role: match msg.role {
                    MessageRole::User => "user".to_string(),
                    MessageRole::Assistant => "assistant".to_string(),
                    MessageRole::System => "system".to_string(),
                },
                content: msg.content,
            })
            .collect()
    }

    /// 事实提取系统提示词
    ///
    /// 引导模型从对话中提取关于用户的事实陈述
    fn fact_extraction_system_prompt() -> &'static str {
        r#"你是一个专业的信息提取专家。请从以下对话中提取出关于用户的事实陈述。

**重要语言约束：所有事实陈述（statement 字段）必须使用中文，禁止使用英语、西班牙语或其他语言。**

要求：
1. 每条事实需要独立、明确、客观
2. 只提取明确的事实，不要进行推断或假设
3. 为每条事实给出置信度评分（0-1）：
   - 1.0：完全确定的事实
   - 0.7-0.9：比较确定的事实
   - 0.4-0.6：可能的事实
   - <0.4：不确定的事实，不要提取

输出格式：JSON 数组，每个元素包含：
- statement: 事实陈述（字符串）
- confidence: 置信度（浮点数，0-1）

示例：
```json
[
  {"statement": "用户喜欢喝咖啡", "confidence": 0.9},
  {"statement": "用户的名字叫小明", "confidence": 1.0}
]
```

现在开始提取："#
    }

    /// 记忆决策系统提示词
    ///
    /// 引导模型决定对新事实应该执行什么操作
    fn memory_decision_system_prompt() -> &'static str {
        r#"你是一个记忆管理专家。请根据新事实和现有的相关记忆，决定应该执行什么操作。

可用操作：
- ADD: 添加新记忆（新事实与现有记忆不相关或补充新信息）
- UPDATE: 更新现有记忆（新事实与现有记忆相关，但内容更准确或更新）
- DELETE: 删除记忆（新事实与现有记忆矛盾）
- NOOP: 无需操作（新事实已存在或无价值）

请只返回操作名称（ADD/UPDATE/DELETE/NOOP），不要其他内容。"#
    }

    /// 解析事实提取结果
    ///
    /// # 参数
    /// - response: LLM 返回的原始响应
    ///
    /// # 返回
    /// 成功返回事实列表，失败返回错误
    fn parse_fact_response(&self, response: &str) -> Result<Vec<Fact>> {
        #[derive(Debug, Deserialize)]
        struct ExtractedFact {
            statement: String,
            confidence: f32,
        }

        // 步骤1: 清理响应，提取 JSON 部分
        let mut json_str = response.trim();

        // 移除 markdown 代码块标记
        json_str = json_str
            .strip_prefix("```json")
            .or_else(|| json_str.strip_prefix("```"))
            .unwrap_or(json_str);

        json_str = json_str.strip_suffix("```").unwrap_or(json_str);

        let json_str = json_str.trim();

        if json_str.is_empty() {
            return Ok(Vec::new());
        }

        // 步骤2: 尝试直接解析 JSON
        if let Ok(extracted) = serde_json::from_str::<Vec<ExtractedFact>>(json_str) {
            let facts: Result<Vec<Fact>> = extracted
                .into_iter()
                .filter(|ef| !ef.statement.trim().is_empty())
                .filter(|ef| ef.confidence >= 0.0 && ef.confidence <= 1.0)
                .map(|ef| {
                    let mut fact = Fact::new(ef.statement, ef.confidence)?;
                    fact.source = FactSource::UserMessage;
                    Ok(fact)
                })
                .collect();
            return facts;
        }

        // 步骤3: 如果直接解析失败，尝试查找第一个 '[' 和最后一个 ']' 之间的内容
        if let (Some(start_idx), Some(end_idx)) = (json_str.find('['), json_str.rfind(']')) {
            if start_idx < end_idx {
                let possible_json = &json_str[start_idx..=end_idx];
                if let Ok(extracted) = serde_json::from_str::<Vec<ExtractedFact>>(possible_json) {
                    let facts: Result<Vec<Fact>> = extracted
                        .into_iter()
                        .filter(|ef| !ef.statement.trim().is_empty())
                        .filter(|ef| ef.confidence >= 0.0 && ef.confidence <= 1.0)
                        .map(|ef| {
                            let mut fact = Fact::new(ef.statement, ef.confidence)?;
                            fact.source = FactSource::UserMessage;
                            Ok(fact)
                        })
                        .collect();
                    return facts;
                }
            }
        }

        // 步骤4: 如果都失败，返回空（比崩溃好）
        Ok(Vec::new())
    }

    /// 解析记忆决策结果
    ///
    /// # 参数
    /// - response: LLM 返回的原始响应
    ///
    /// # 返回
    /// 成功返回记忆操作，失败返回默认操作（Add）
    fn parse_action_response(&self, response: &str) -> Result<MemoryAction> {
        let response = response.trim().to_uppercase();

        let action = match response.as_str() {
            "ADD" => MemoryAction::Add,
            "UPDATE" => MemoryAction::Update,
            "DELETE" => MemoryAction::Delete,
            "NOOP" | "NO OP" | "NO_OP" => MemoryAction::Noop,
            s if s.contains("ADD") => MemoryAction::Add,
            s if s.contains("UPDATE") => MemoryAction::Update,
            s if s.contains("DELETE") => MemoryAction::Delete,
            s if s.contains("NOOP") || s.contains("NO OP") => MemoryAction::Noop,
            _ => MemoryAction::Add,
        };

        Ok(action)
    }
}

#[async_trait]
impl LlmClient for OllamaLlmClient {
    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        Self::validate_message_count(&messages)?;

        for msg in &messages {
            Self::validate_input_length(&msg.content)?;
        }

        if !Self::is_valid_url(&self.base_url) {
            return Err(ShouxinError::LLMError(format!(
                "无效的 URL: {}，只允许 http/https 协议",
                self.base_url
            )));
        }

        let ollama_messages = self.convert_messages(messages);
        let request = self.build_chat_request(ollama_messages);

        let url = format!("{}/api/chat", self.base_url);

        let response: OllamaChatResponse = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ShouxinError::LLMError(format!(
                        "Ollama 请求超时（{}秒）: {}",
                        DEFAULT_TIMEOUT_SECS, e
                    ))
                } else if e.is_connect() {
                    ShouxinError::LLMError(format!(
                        "无法连接到 Ollama 服务（{}）: {}",
                        self.base_url, e
                    ))
                } else {
                    ShouxinError::LLMError(format!("Ollama 聊天请求失败: {}", e))
                }
            })?
            .json()
            .await
            .map_err(|e| ShouxinError::LLMError(format!("解析 Ollama 响应失败: {}", e)))?;

        let content = response
            .message
            .ok_or_else(|| ShouxinError::LLMError("Ollama 返回空响应".to_string()))?
            .content;

        Ok(content)
    }

    async fn extract_facts(&self, conversation: &str) -> Result<Vec<Fact>> {
        Self::validate_input_length(conversation)?;

        let system_prompt = Message::system(Self::fact_extraction_system_prompt())?;
        let user_prompt = Message::user(conversation)?;

        let messages = vec![system_prompt, user_prompt];
        let response = self.chat(messages).await?;

        self.parse_fact_response(&response)
    }

    async fn decide_memory_action(
        &self,
        new_fact: &Fact,
        existing_memories: Vec<&Memory>,
    ) -> Result<MemoryAction> {
        Self::validate_input_length(&new_fact.statement)?;

        let mut prompt = String::new();
        prompt.push_str("新事实：\n");
        prompt.push_str(&format!("- {}\n\n", new_fact.statement));

        if !existing_memories.is_empty() {
            prompt.push_str("现有相关记忆：\n");
            for (i, mem) in existing_memories.iter().enumerate() {
                prompt.push_str(&format!("{}. {}\n", i + 1, mem.content));
            }
        } else {
            prompt.push_str("现有相关记忆：无\n");
        }

        Self::validate_input_length(&prompt)?;

        let system_prompt = Message::system(Self::memory_decision_system_prompt())?;
        let user_prompt = Message::user(prompt)?;

        let messages = vec![system_prompt, user_prompt];
        let response = self.chat(messages).await?;

        self.parse_action_response(&response)
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        Self::validate_input_length(prompt)?;

        let messages = vec![Message::user(prompt)?];
        self.chat(messages).await
    }
}
