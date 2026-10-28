//! LLM 配置

use serde::Deserialize;

/// LLM 配置
#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    /// Ollama API URL
    pub url: String,

    /// 模型名称
    pub model: String,

    /// 温度参数
    pub temperature: f32,

    /// 最大 token 数
    pub max_tokens: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:11434".to_string(),
            model: "llama3.2:3b".to_string(),
            temperature: 0.7,
            max_tokens: 1024,
        }
    }
}
