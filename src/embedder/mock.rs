//! 模拟嵌入器实现

use async_trait::async_trait;

use crate::core::Embedding;
use crate::error::ShouxinError;
use crate::traits::embedder::Embedder;

/// 模拟嵌入器实现
/// 
/// 用于测试和开发环境，不需要下载实际的嵌入模型
pub struct MockEmbedder {
    dimension: usize,
    model_name: String,
}

impl MockEmbedder {
    /// 创建新的模拟嵌入器
    pub fn new(dimension: usize, model_name: &str) -> Self {
        Self {
            dimension,
            model_name: model_name.to_string(),
        }
    }
    
    /// 创建默认的模拟嵌入器（768维）
    pub fn new_default() -> Self {
        Self::new(768, "mock-embedder")
    }
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, _text: &str) -> Result<Embedding, ShouxinError> {
        // 生成随机嵌入向量
        let vector = vec![0.0; self.dimension];
        
        Ok(Embedding {
            vector,
            dimension: self.dimension,
            model_name: self.model_name.clone(),
        })
    }
    
    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Embedding>, ShouxinError> {
        Ok(texts
            .iter()
            .map(|_| {
                let vector = vec![0.0; self.dimension];
                Embedding {
                    vector,
                    dimension: self.dimension,
                    model_name: self.model_name.clone(),
                }
            })
            .collect())
    }
    
    fn dimension(&self) -> usize {
        self.dimension
    }
    
    fn model_name(&self) -> &str {
        &self.model_name
    }
}