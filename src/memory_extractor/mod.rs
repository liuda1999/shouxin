//! 内存提取器模块
//!
//! 通用 LLM 内存提取器，支持多源后端。
//! 保留 ollama_extractor 作为向后兼容。

pub mod extractor;
pub mod ollama_extractor;
pub mod dispatcher;

pub use extractor::{
    ElementType, ExtractedElements, ExtractedEvent, ExtractedFact, ExtractedLearning,
    ExtractedPerson, ExtractedTodo, LlmMemoryExtractor, MemoryExtractor, SimpleExtractor,
    TodoUpdateIntent,
};
pub use ollama_extractor::OllamaMemoryExtractor;
pub use dispatcher::{DataDispatcher, DataDispatcherTrait, DispatchInput};