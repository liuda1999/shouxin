//! OpenAI 兼容 API 客户端实现
//!
//! 通过 reqwest 调用远程 HTTP API，支持 OpenAI 格式及自定义端点

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::core::{Fact, FactSource, Memory, MemoryAction, Message, MessageRole};
use crate::error::{Result, ShouxinError};
use crate::traits::llm::{LlmClient, LlmConfig};

#[derive(Debug, Serialize)]
struct ApiChatRequest {
    model: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ApiChatResponse {
    choices: Vec<ApiChoice>,
}

#[derive(Debug, Deserialize)]
struct ApiChoice {
    message: ApiMessage,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    error: Option<ApiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

pub struct ApiClient {
    client: Client,
    endpoint: String,
    api_key: String,
    model: String,
    temperature: f32,
    max_tokens: Option<u32>,
    timeout_secs: u64,
    max_retries: u32,
    extra_headers: HashMap<String, String>,
    request_count: AtomicU64,
    fail_count: AtomicU64,
}

impl ApiClient {
    pub fn new(config: LlmConfig) -> Self {
        let endpoint = config.endpoint.unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());
        let api_key = config.extra_params.get("api_key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let timeout_secs = config.extra_params.get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);
        let max_retries = config.extra_params.get("max_retries")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as u32;

        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            endpoint,
            api_key,
            model: config.model,
            temperature: config.temperature.clamp(0.0, 2.0),
            max_tokens: config.max_tokens,
            timeout_secs,
            max_retries,
            extra_headers: HashMap::new(),
            request_count: AtomicU64::new(0),
            fail_count: AtomicU64::new(0),
        }
    }

    pub fn with_extra_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.extra_headers = headers;
        self
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if !self.api_key.is_empty() {
            let auth_value = format!("Bearer {}", self.api_key);
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&auth_value)
                    .map_err(|e| ShouxinError::LLMError(format!("无效的 API key: {}", e)))?,
            );
        }

        for (key, value) in &self.extra_headers {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(key.as_bytes())
                    .map_err(|e| ShouxinError::LLMError(format!("无效的 header 名: {}", e)))?,
                HeaderValue::from_str(value)
                    .map_err(|e| ShouxinError::LLMError(format!("无效的 header 值: {}", e)))?,
            );
        }

        Ok(headers)
    }

    fn convert_messages(&self, messages: Vec<Message>) -> Vec<ApiMessage> {
        messages
            .into_iter()
            .map(|msg| ApiMessage {
                role: match msg.role {
                    MessageRole::User => "user".to_string(),
                    MessageRole::Assistant => "assistant".to_string(),
                    MessageRole::System => "system".to_string(),
                },
                content: msg.content,
            })
            .collect()
    }

    async fn send_request(&self, messages: Vec<ApiMessage>) -> Result<String> {
        let request = ApiChatRequest {
            model: self.model.clone(),
            messages,
            temperature: Some(self.temperature),
            max_tokens: self.max_tokens,
            stream: false,
        };

        let headers = self.build_headers()?;

        self.request_count.fetch_add(1, Ordering::Relaxed);

        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }

            let response = self
                .client
                .post(&self.endpoint)
                .headers(headers.clone())
                .json(&request)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();

                    if status.is_success() {
                        match resp.json::<ApiChatResponse>().await {
                            Ok(chat_resp) => {
                                if let Some(choice) = chat_resp.choices.first() {
                                    return Ok(choice.message.content.clone());
                                }
                                last_error = Some(ShouxinError::LLMError(
                                    "API 返回空响应".to_string(),
                                ));
                            }
                            Err(e) => {
                                last_error = Some(ShouxinError::LLMError(format!(
                                    "解析 API 响应失败: {}", e
                                )));
                                continue;
                            }
                        }
                    } else {
                        let error_body = resp.text().await.unwrap_or_default();
                        let error_msg = serde_json::from_str::<ApiErrorResponse>(&error_body)
                            .ok()
                            .and_then(|e| e.error)
                            .map(|e| e.message)
                            .unwrap_or_else(|| format!("HTTP {}", status));

                        last_error = Some(ShouxinError::LLMError(format!(
                            "API 请求失败 ({}) [尝试 {}/{}]: {}",
                            status,
                            attempt + 1,
                            self.max_retries + 1,
                            error_msg
                        )));

                        if status.is_server_error() {
                            continue;
                        }
                        break;
                    }
                }
                Err(e) => {
                    last_error = Some(ShouxinError::LLMError(format!(
                        "API 网络错误 [尝试 {}/{}]: {}",
                        attempt + 1,
                        self.max_retries + 1,
                        e
                    )));
                    continue;
                }
            }
        }

        self.fail_count.fetch_add(1, Ordering::Relaxed);
        Err(last_error.unwrap_or_else(|| ShouxinError::LLMError("API 请求失败: 未知错误".to_string())))
    }

    pub fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }

    pub fn fail_count(&self) -> u64 {
        self.fail_count.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl LlmClient for ApiClient {
    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        let api_messages = self.convert_messages(messages);
        self.send_request(api_messages).await
    }

    async fn extract_facts(&self, conversation: &str) -> Result<Vec<Fact>> {
        let system_prompt = r#"你是一个专业的信息提取专家。请从以下对话中提取关于用户的事实陈述，返回 JSON 数组。
格式：[{"statement": "事实", "confidence": 0.9}]
只提取明确的事实，置信度 >= 0.7。

**重要语言约束：statement 字段必须使用中文，禁止使用英语、西班牙语或其他语言。**"#;

        let messages = vec![
            Message::system(system_prompt)?,
            Message::user(conversation)?,
        ];

        let response = self.chat(messages).await?;
        Self::parse_fact_json(&response)
    }

    async fn decide_memory_action(
        &self,
        new_fact: &Fact,
        existing_memories: Vec<&Memory>,
    ) -> Result<MemoryAction> {
        let mut prompt = String::from("新事实:\n");
        prompt.push_str(&format!("- {}\n", new_fact.statement));
        prompt.push_str("\n现有记忆:\n");
        if existing_memories.is_empty() {
            prompt.push_str("无\n");
        } else {
            for (i, mem) in existing_memories.iter().enumerate() {
                prompt.push_str(&format!("{}. {}\n", i + 1, mem.content));
            }
        }
        prompt.push_str("\n请决定操作: ADD, UPDATE, DELETE, 或 NOOP");

        let messages = vec![
            Message::system("你是记忆管理专家。根据新事实和现有记忆决定操作。只返回 ADD/UPDATE/DELETE/NOOP.")?,
            Message::user(&prompt)?,
        ];

        let response = self.chat(messages).await?;
        let action = response.trim().to_uppercase();

        Ok(match action.as_str() {
            "ADD" => MemoryAction::Add,
            "UPDATE" => MemoryAction::Update,
            "DELETE" => MemoryAction::Delete,
            "NOOP" | "NO OP" | "NO_OP" => MemoryAction::Noop,
            s if s.contains("ADD") => MemoryAction::Add,
            s if s.contains("UPDATE") => MemoryAction::Update,
            s if s.contains("DELETE") => MemoryAction::Delete,
            _ => MemoryAction::Add,
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        let messages = vec![Message::user(prompt)?];
        self.chat(messages).await
    }
}

impl ApiClient {
    fn parse_fact_json(response: &str) -> Result<Vec<Fact>> {
        #[derive(Debug, Deserialize)]
        struct RawFact {
            statement: String,
            confidence: f32,
        }

        let json_str = response
            .strip_prefix("```json")
            .or_else(|| response.strip_prefix("```"))
            .unwrap_or(response);
        let json_str = json_str.strip_suffix("```").unwrap_or(json_str).trim();

        if json_str.is_empty() {
            return Ok(Vec::new());
        }

        let facts: Vec<RawFact> = if let (Some(s), Some(e)) = (json_str.find('['), json_str.rfind(']')) {
            serde_json::from_str(&json_str[s..=e]).unwrap_or_default()
        } else {
            serde_json::from_str(json_str).unwrap_or_default()
        };

        Ok(facts
            .into_iter()
            .filter(|f| !f.statement.trim().is_empty() && f.confidence >= 0.7)
            .map(|f| {
                let mut fact = Fact::new(f.statement, f.confidence).unwrap_or_else(|_| {
                    Fact::new("unknown".to_string(), 0.5).unwrap()
                });
                fact.source = FactSource::UserMessage;
                fact
            })
            .collect())
    }
}