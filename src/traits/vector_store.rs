//! 向量存储 Trait
//!
//! 定义向量存储的标准接口

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::core::{Embedding, Memory, MemoryFilter, SearchResult};
use crate::error::ShouxinError;

/// 向量存储 Trait
///
/// 定义向量存储的标准接口
/// 支持多种后端（如 sled、Qdrant、Pinecone 等）
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// 插入记忆
    ///
    /// # 参数
    /// - `memory`: 要插入的记忆
    ///
    /// # 返回
    /// 操作结果
    ///
    /// # 说明
    /// 记忆必须包含嵌入向量才能被正确索引
    ///
    /// # 示例
    /// ```ignore
    /// let memory = Memory::new("用户喜欢咖啡")
    ///     .with_embedding(embedding);
    /// store.insert(memory).await?;
    /// ```
    async fn insert(&self, memory: Memory) -> Result<(), ShouxinError>;

    /// 批量插入记忆
    ///
    /// # 参数
    /// - `memories`: 记忆列表
    ///
    /// # 返回
    /// 操作结果
    async fn insert_batch(&self, memories: Vec<Memory>) -> Result<(), ShouxinError> {
        for memory in memories {
            self.insert(memory).await?;
        }
        Ok(())
    }

    /// 搜索相似记忆
    ///
    /// # 参数
    /// - `query_embedding`: 查询嵌入向量
    /// - `limit`: 返回结果数量限制
    /// - `filters`: 可选的过滤条件
    ///
    /// # 返回
    /// 搜索结果列表，按相似度降序排列
    ///
    /// # 示例
    /// ```ignore
    /// let query_embedding = embedder.embed("咖啡").await?;
    /// let results = store.search(&query_embedding, 10, None).await?;
    /// for result in results {
    ///     println!("相似度: {}, 内容: {}", result.score, result.memory.content);
    /// }
    /// ```
    async fn search(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        filters: Option<HashMap<String, Value>>,
    ) -> Result<Vec<SearchResult>, ShouxinError>;

    /// 带过滤器的搜索
    ///
    /// # 参数
    /// - `query_embedding`: 查询嵌入向量
    /// - `filter`: 记忆过滤器
    /// - `limit`: 返回结果数量限制
    ///
    /// # 返回
    /// 搜索结果列表
    async fn search_with_filter(
        &self,
        query_embedding: &Embedding,
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<SearchResult>, ShouxinError>;

    /// 删除记忆
    ///
    /// # 参数
    /// - `memory_id`: 记忆 ID
    ///
    /// # 返回
    /// 操作结果
    async fn delete(&self, memory_id: &str) -> Result<(), ShouxinError>;

    /// 批量删除记忆
    ///
    /// # 参数
    /// - `memory_ids`: 记忆 ID 列表
    ///
    /// # 返回
    /// 操作结果
    async fn delete_batch(&self, memory_ids: Vec<String>) -> Result<(), ShouxinError> {
        for id in memory_ids {
            self.delete(&id).await?;
        }
        Ok(())
    }

    /// 更新记忆
    ///
    /// # 参数
    /// - `memory`: 更新后的记忆
    ///
    /// # 返回
    /// 操作结果
    async fn update(&self, memory: Memory) -> Result<(), ShouxinError>;

    /// 获取单个记忆
    ///
    /// # 参数
    /// - `memory_id`: 记忆 ID
    ///
    /// # 返回
    /// 记忆对象，如果不存在则返回 None
    async fn get(&self, memory_id: &str) -> Result<Option<Memory>, ShouxinError>;

    /// 获取所有记忆
    ///
    /// # 参数
    /// - `filter`: 可选的过滤条件
    ///
    /// # 返回
    /// 记忆列表
    async fn list(&self, filter: Option<&MemoryFilter>) -> Result<Vec<Memory>, ShouxinError>;

    /// 获取记忆数量
    ///
    /// # 参数
    /// - `filter`: 可选的过滤条件
    ///
    /// # 返回
    /// 记忆数量
    async fn count(&self, filter: Option<&MemoryFilter>) -> Result<usize, ShouxinError>;

    /// 清空所有记忆
    async fn clear(&self) -> Result<(), ShouxinError>;

    /// 检查记忆是否存在
    ///
    /// # 参数
    /// - `memory_id`: 记忆 ID
    ///
    /// # 返回
    /// 是否存在
    async fn exists(&self, memory_id: &str) -> Result<bool, ShouxinError> {
        Ok(self.get(memory_id).await?.is_some())
    }
}

/// 向量存储配置
///
/// 向量存储的配置参数
#[derive(Debug, Clone)]
pub struct VectorStoreConfig {
    /// 存储路径
    ///
    /// 持久化存储的文件路径
    pub path: Option<String>,

    /// 嵌入维度
    ///
    /// 向量的维度，必须与嵌入模型匹配
    pub dimension: usize,

    /// 距离度量类型
    pub distance_metric: DistanceMetric,

    /// 是否使用内存模式
    ///
    /// 内存模式下数据不会持久化
    pub in_memory: bool,
}

impl Default for VectorStoreConfig {
    fn default() -> Self {
        Self {
            path: None,
            dimension: 384,
            distance_metric: DistanceMetric::Cosine,
            in_memory: true,
        }
    }
}

impl VectorStoreConfig {
    /// 创建内存模式配置
    pub fn in_memory() -> Self {
        Self {
            in_memory: true,
            ..Default::default()
        }
    }

    /// 创建持久化配置
    pub fn persistent(path: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            in_memory: false,
            ..Default::default()
        }
    }

    /// 设置嵌入维度
    pub fn with_dimension(mut self, dimension: usize) -> Self {
        self.dimension = dimension;
        self
    }

    /// 设置距离度量
    pub fn with_distance_metric(mut self, metric: DistanceMetric) -> Self {
        self.distance_metric = metric;
        self
    }
}

/// 距离度量类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceMetric {
    /// 余弦相似度
    Cosine,
    /// 欧几里得距离
    Euclidean,
    /// 点积
    DotProduct,
    /// 曼哈顿距离
    Manhattan,
}

impl DistanceMetric {
    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            DistanceMetric::Cosine => "cosine",
            DistanceMetric::Euclidean => "euclidean",
            DistanceMetric::DotProduct => "dot",
            DistanceMetric::Manhattan => "manhattan",
        }
    }
}
