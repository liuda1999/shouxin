//! 应用配置

use serde::Deserialize;

use crate::config::{EmbedderConfig, LlmConfig, VectorStoreConfig};

/// 应用配置
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    /// LLM 配置
    pub llm: LlmConfig,

    /// Embedding 配置
    pub embedder: EmbedderConfig,

    /// 向量存储配置
    pub vector_store: VectorStoreConfig,

    /// 数据目录
    pub data_dir: String,

    /// 字典库路径
    pub dictionary_path: Option<String>,

    /// 关系库路径
    pub relational_path: Option<String>,

    /// 向量库路径
    pub vector_path: Option<String>,

    /// 日志级别
    pub log_level: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            llm: LlmConfig::default(),
            embedder: EmbedderConfig::default(),
            vector_store: VectorStoreConfig::default(),
            data_dir: "./data".to_string(),
            dictionary_path: None,
            relational_path: None,
            vector_path: None,
            log_level: "info".to_string(),
        }
    }
}
