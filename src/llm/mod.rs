//! LLM 客户端模块
//!
//! 支持多源 LLM 后端：Ollama / GGUF (llama.cpp) / API (OpenAI 兼容)
//! 通过 LlmClientManager 实现主备切换和健康检查

pub mod ollama;
pub mod api;
#[cfg(feature = "gguf")]
pub mod gguf;
pub mod manager;
pub mod factory;

pub use ollama::OllamaLlmClient;
pub use api::ApiClient;
#[cfg(feature = "gguf")]
pub use gguf::GgufClient;
pub use manager::{LlmClientManager, LlmSourceType, ManagerStatus};
pub use factory::build_llm_manager;