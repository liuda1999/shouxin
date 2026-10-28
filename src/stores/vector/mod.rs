//! 向量库（Vector Store）
//!
//! 封装现有的向量存储，保持功能不变

pub mod factory;
pub mod hnsw_store;
pub mod store;
pub mod types;

use std::sync::Arc;

use crate::core::{Embedding, Memory, MemoryFilter, SearchResult};
use crate::error::ShouxinError;
use crate::traits::VectorStore;

/// 向量库封装
pub struct VectorStoreWrapper {
    inner: Arc<dyn VectorStore>,
}

impl VectorStoreWrapper {
    /// 创建新的向量库封装
    pub fn new(inner: Arc<dyn VectorStore>) -> Self {
        Self { inner }
    }

    /// 获取内部向量存储
    pub fn inner(&self) -> &Arc<dyn VectorStore> {
        &self.inner
    }

    /// 插入记忆
    pub async fn insert(&self, memory: Memory) -> Result<(), ShouxinError> {
        self.inner.insert(memory).await
    }

    /// 批量插入
    pub async fn insert_batch(&self, memories: Vec<Memory>) -> Result<(), ShouxinError> {
        self.inner.insert_batch(memories).await
    }

    /// 搜索相似记忆
    pub async fn search(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        filters: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<Vec<SearchResult>, ShouxinError> {
        self.inner.search(query_embedding, limit, filters).await
    }

    /// 搜索（带过滤器）
    pub async fn search_with_filter(
        &self,
        query_embedding: &Embedding,
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<SearchResult>, ShouxinError> {
        self.inner
            .search_with_filter(query_embedding, filter, limit)
            .await
    }

    /// 更新记忆
    pub async fn update(&self, memory: Memory) -> Result<(), ShouxinError> {
        self.inner.update(memory).await
    }

    /// 删除记忆
    pub async fn delete(&self, memory_id: &str) -> Result<(), ShouxinError> {
        self.inner.delete(memory_id).await
    }

    /// 批量删除
    pub async fn delete_batch(&self, memory_ids: Vec<String>) -> Result<(), ShouxinError> {
        self.inner.delete_batch(memory_ids).await
    }

    /// 获取记忆
    pub async fn get(&self, memory_id: &str) -> Result<Option<Memory>, ShouxinError> {
        self.inner.get(memory_id).await
    }

    /// 获取所有记忆
    pub async fn list(&self, filter: Option<&MemoryFilter>) -> Result<Vec<Memory>, ShouxinError> {
        self.inner.list(filter).await
    }

    /// 获取记忆数量
    pub async fn count(&self, filter: Option<&MemoryFilter>) -> Result<usize, ShouxinError> {
        self.inner.count(filter).await
    }

    /// 清空所有记忆
    pub async fn clear(&self) -> Result<(), ShouxinError> {
        self.inner.clear().await
    }

    /// 检查记忆是否存在
    pub async fn exists(&self, memory_id: &str) -> Result<bool, ShouxinError> {
        self.inner.exists(memory_id).await
    }
}
