//! 嵌入器 Trait
//!
//! 定义文本嵌入生成的标准接口

use async_trait::async_trait;

use crate::core::Embedding;
use crate::error::ShouxinError;

/// 嵌入器 Trait
///
/// 定义文本嵌入生成的标准接口
/// 支持多种嵌入模型（如 fastembed、OpenAI 等）
#[async_trait]
pub trait Embedder: Send + Sync {
    /// 生成单个文本的嵌入向量
    ///
    /// # 参数
    /// - `text`: 输入文本
    ///
    /// # 返回
    /// 文本的嵌入向量表示
    ///
    /// # 示例
    /// ```ignore
    /// let embedding = embedder.embed("你好世界").await?;
    /// println!("向量维度: {}", embedding.dimension);
    /// ```
    async fn embed(&self, text: &str) -> Result<Embedding, ShouxinError>;

    /// 批量生成嵌入向量
    ///
    /// # 参数
    /// - `texts`: 文本列表
    ///
    /// # 返回
    /// 对应的嵌入向量列表
    ///
    /// # 说明
    /// 批量处理通常比逐个处理更高效
    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Embedding>, ShouxinError>;

    /// 获取嵌入维度
    ///
    /// # 返回
    /// 嵌入向量的维度
    fn dimension(&self) -> usize;

    /// 获取模型名称
    ///
    /// # 返回
    /// 当前使用的嵌入模型名称
    fn model_name(&self) -> &str;

    /// 获取最大输入长度
    ///
    /// # 返回
    /// 模型支持的最大输入文本长度（字符数或令牌数）
    fn max_input_length(&self) -> usize {
        512
    }
}

/// 嵌入器配置
///
/// 嵌入器的配置参数
#[derive(Debug, Clone)]
pub struct EmbedderConfig {
    /// 模型名称
    pub model: String,

    /// 批处理大小
    ///
    /// 批量嵌入时的批次大小
    pub batch_size: usize,

    /// 是否归一化向量
    ///
    /// 归一化后的向量余弦相似度计算更高效
    pub normalize: bool,

    /// 最大序列长度
    ///
    /// 模型支持的最大序列长度
    pub max_sequence_length: usize,
}

impl Default for EmbedderConfig {
    fn default() -> Self {
        Self {
            model: "BAAI/bge-base-en-v1.5".to_string(),
            batch_size: 32,
            normalize: true,
            max_sequence_length: 512,
        }
    }
}

impl EmbedderConfig {
    /// 创建新的配置
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Default::default()
        }
    }

    /// 设置批处理大小
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// 设置是否归一化
    pub fn with_normalize(mut self, normalize: bool) -> Self {
        self.normalize = normalize;
        self
    }

    /// 设置最大序列长度
    pub fn with_max_sequence_length(mut self, length: usize) -> Self {
        self.max_sequence_length = length;
        self
    }
}

/// 同步嵌入器 Trait
///
/// 用于不支持异步的嵌入模型
pub trait SyncEmbedder: Send + Sync {
    /// 同步生成嵌入向量
    fn embed_sync(&self, text: &str) -> Result<Embedding, ShouxinError>;

    /// 同步批量生成嵌入向量
    fn embed_batch_sync(&self, texts: Vec<String>) -> Result<Vec<Embedding>, ShouxinError>;

    /// 获取嵌入维度
    fn dimension(&self) -> usize;

    /// 获取模型名称
    fn model_name(&self) -> &str;
}
