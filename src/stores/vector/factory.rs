//! 向量库工厂
//!
//! 提供向量库实例的创建和管理，支持多种实现（嵌入式、模拟等）
//!
//! # 工厂模式优势
//! 1. 解耦调用方与具体实现
//! 2. 配置驱动，运行时动态选择
//! 3. 统一初始化复杂度管理
//! 4. 便于单元测试和集成测试
//! 5. 支持未来扩展
//! 6. 单例/缓存控制（可选）

use once_cell::sync::OnceCell;
use std::sync::Arc;

use crate::error::ShouxinError;
use crate::stores::vector::hnsw_store::HnswVectorStore;
use crate::stores::vector::store::EmbeddedVectorStore;
use crate::stores::vector::types::VectorIndexConfig;
use crate::traits::VectorStore;

/// 向量库配置
#[derive(Debug, Clone)]
pub struct VectorStoreConfig {
    /// 存储路径
    pub path: String,
    /// 向量库类型
    pub store_type: VectorStoreType,
    /// 向量维度
    pub dimension: usize,
    /// 索引配置
    pub index_config: VectorIndexConfig,
}

impl Default for VectorStoreConfig {
    fn default() -> Self {
        Self {
            path: "./data/vector_store".to_string(),
            store_type: VectorStoreType::default(),
            dimension: 768,
            index_config: VectorIndexConfig::default(),
        }
    }
}

/// 向量库类型
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum VectorStoreType {
    /// 嵌入式向量库（默认）
    #[default]
    Embedded,
    /// HNSW 向量库（高性能近似最近邻搜索）
    Hnsw,
    /// 模拟向量库（用于测试）
    Mock,
}

/// 向量库工厂
///
/// 提供向量库实例的创建和管理
pub struct VectorStoreFactory;

impl VectorStoreFactory {
    /// 创建 HNSW 向量库（返回具体类型）
    pub fn create_hnsw_concrete(config: &VectorStoreConfig) -> Result<Arc<HnswVectorStore>, ShouxinError> {
        let store = HnswVectorStore::new(&config.path, config.dimension, config.index_config.clone())?;
        Ok(Arc::new(store))
    }

    /// 创建 HNSW 向量库
    pub fn create_hnsw(config: &VectorStoreConfig) -> Result<Arc<dyn VectorStore>, ShouxinError> {
        let store = HnswVectorStore::new(&config.path, config.dimension, config.index_config.clone())?;
        Ok(Arc::new(store))
    }

    /// 创建嵌入式向量库（返回具体类型）
    pub fn create_embedded_concrete(config: &VectorStoreConfig) -> Result<Arc<EmbeddedVectorStore>, ShouxinError> {
        let store = EmbeddedVectorStore::new_with_dimension(&config.path, config.dimension)?;
        Ok(Arc::new(store))
    }

    /// 创建嵌入式向量库
    pub fn create_embedded(config: &VectorStoreConfig) -> Result<Arc<dyn VectorStore>, ShouxinError> {
        let store = EmbeddedVectorStore::new_with_dimension(&config.path, config.dimension)?;
        Ok(Arc::new(store))
    }

    /// 根据配置创建向量库
    ///
    /// # 参数
    /// - `config`: 向量库配置
    ///
    /// # 返回
    /// 向量库实例
    pub fn create(config: &VectorStoreConfig) -> Result<Arc<dyn VectorStore>, ShouxinError> {
        match config.store_type {
            VectorStoreType::Embedded => Self::create_embedded(config),
            VectorStoreType::Hnsw => Self::create_hnsw(config),
            VectorStoreType::Mock => Self::create_mock(),
        }
    }

    /// 创建模拟向量库（用于测试）
    ///
    /// # 返回
    /// 模拟向量库实例
    pub fn create_mock() -> Result<Arc<dyn VectorStore>, ShouxinError> {
        struct MockVectorStore;

        #[async_trait::async_trait]
        impl VectorStore for MockVectorStore {
            async fn insert(&self, _memory: crate::core::Memory) -> Result<(), ShouxinError> {
                Ok(())
            }

            async fn search(
                &self,
                _query_embedding: &crate::core::Embedding,
                _limit: usize,
                _filters: Option<std::collections::HashMap<String, serde_json::Value>>,
            ) -> Result<Vec<crate::core::SearchResult>, ShouxinError> {
                Ok(Vec::new())
            }

            async fn search_with_filter(
                &self,
                _query_embedding: &crate::core::Embedding,
                _filter: &crate::core::MemoryFilter,
                _limit: usize,
            ) -> Result<Vec<crate::core::SearchResult>, ShouxinError> {
                Ok(Vec::new())
            }

            async fn delete(&self, _memory_id: &str) -> Result<(), ShouxinError> {
                Ok(())
            }

            async fn update(&self, _memory: crate::core::Memory) -> Result<(), ShouxinError> {
                Ok(())
            }

            async fn get(
                &self,
                _memory_id: &str,
            ) -> Result<Option<crate::core::Memory>, ShouxinError> {
                Ok(None)
            }

            async fn list(
                &self,
                _filter: Option<&crate::core::MemoryFilter>,
            ) -> Result<Vec<crate::core::Memory>, ShouxinError> {
                Ok(Vec::new())
            }

            async fn count(
                &self,
                _filter: Option<&crate::core::MemoryFilter>,
            ) -> Result<usize, ShouxinError> {
                Ok(0)
            }

            async fn clear(&self) -> Result<(), ShouxinError> {
                Ok(())
            }
        }

        Ok(Arc::new(MockVectorStore))
    }
}

/// 全局向量库单例
///
/// 确保全局使用同一个向量库连接，避免重复加载模型和索引文件
static GLOBAL_VECTOR_STORE: OnceCell<Arc<dyn VectorStore>> = OnceCell::new();

impl VectorStoreFactory {
    /// 获取全局向量库单例
    ///
    /// # 参数
    /// - `config`: 向量库配置（仅在首次调用时使用）
    ///
    /// # 返回
    /// 全局向量库单例
    pub fn get_global(config: &VectorStoreConfig) -> Result<Arc<dyn VectorStore>, ShouxinError> {
        GLOBAL_VECTOR_STORE
            .get_or_try_init(|| Self::create(config))
            .cloned()
    }

    /// 重置全局向量库单例（仅用于测试）
    #[cfg(test)]
    pub fn reset_global() {
        // OnceCell 没有重置方法，实际项目中可能需要使用其他方式
        // 这里我们简单地忽略重置操作
    }
}

impl VectorStoreConfig {
    /// 创建新的向量库配置
    ///
    /// # 参数
    /// - `path`: 存储路径
    ///
    /// # 返回
    /// 向量库配置
    pub fn new(path: String) -> Self {
        Self {
            path,
            ..Default::default()
        }
    }

    /// 设置向量库类型
    pub fn with_store_type(mut self, store_type: VectorStoreType) -> Self {
        self.store_type = store_type;
        self
    }

    /// 设置向量维度
    pub fn with_dimension(mut self, dimension: usize) -> Self {
        self.dimension = dimension;
        self
    }

    /// 设置索引配置
    pub fn with_index_config(mut self, index_config: VectorIndexConfig) -> Self {
        self.index_config = index_config;
        self
    }
}
