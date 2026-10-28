//! Embedding 配置

use serde::Deserialize;

/// Embedding 配置
#[derive(Debug, Clone, Deserialize)]
pub struct EmbedderConfig {
    /// 模型名称
    ///
    /// 推荐使用 BAAI/bge-base-en-v1.5 模型，768 维向量
    /// 该模型在语义搜索和文本嵌入任务中表现更好
    pub model: String,

    /// 嵌入维度
    ///
    /// BAAI/bge-base-en-v1.5 模型的维度为 768
    pub dimension: usize,

    /// 批处理大小
    ///
    /// 较大的批处理大小可以提高处理速度，但会增加内存使用
    pub batch_size: usize,
}

impl Default for EmbedderConfig {
    fn default() -> Self {
        Self {
            model: "BAAI/bge-base-en-v1.5".to_string(),
            dimension: 768,
            batch_size: 32,
        }
    }
}
