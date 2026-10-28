//! Trait 定义模块
//!
//! 包含所有可扩展接口的定义

pub mod embedder;
pub mod llm;
pub mod store_traits;
pub mod vector_store;

pub use embedder::{Embedder, EmbedderConfig, SyncEmbedder};
pub use llm::{LlmClient, LlmConfig};
pub use store_traits::{
    ArchiveRecord, ArchiveType, CleanupRule, ContactIndex, ContactRecord, EventIndex,
};
pub use vector_store::{DistanceMetric, VectorStore, VectorStoreConfig};
