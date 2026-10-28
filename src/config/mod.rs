//! 配置系统
//!
//! 管理应用配置，支持从 config.toml 或环境变量加载

pub mod app;
pub mod embedder;
pub mod llm;
pub mod settings;
pub mod vector_store;

pub use app::AppConfig;
pub use embedder::EmbedderConfig;
pub use llm::LlmConfig;
pub use settings::{EventSettings, ExtractionSettings, Settings, TodoSettings};
pub use vector_store::VectorStoreConfig;
