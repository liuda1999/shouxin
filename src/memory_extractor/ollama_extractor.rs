//! 向后兼容：Ollama 记忆提取器
//!
//! 将新的通用 LlmMemoryExtractor 重新导出为 OllamaMemoryExtractor

pub use super::extractor::LlmMemoryExtractor as OllamaMemoryExtractor;