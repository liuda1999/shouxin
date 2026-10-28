//! 系统初始化模块
//!
//! 提供统一的初始化入口，集成三库架构和向量库

use std::sync::Arc;

use crate::error::ShouxinError;
use crate::memory::ThreeLibraryMemoryManager;
use crate::memory::any_vector_store::AnyVectorStore;
use crate::memory_extractor::extractor::MemoryExtractor;
use crate::memory_extractor::DataDispatcher;
use crate::stores::dictionary::store::DictionaryStore;
use crate::stores::relational::RelationalStore;
use crate::stores::vector::factory::{VectorStoreConfig, VectorStoreFactory, VectorStoreType};
use crate::stores::vector::store::EmbeddedVectorStore;
use crate::stores::vector::hnsw_store::HnswVectorStore;
use crate::traits::{Embedder, LlmClient, VectorStore};

/// 系统配置
#[derive(Debug, Clone)]
pub struct SystemConfig {
    /// 字典库路径
    pub dictionary_path: String,
    /// 关系库路径
    pub relational_path: String,
    /// 向量库配置
    pub vector_config: VectorStoreConfig,
    /// 是否使用内存模式
    pub in_memory: bool,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            dictionary_path: "./data/dictionary".to_string(),
            relational_path: "./data/relational.db".to_string(),
            vector_config: VectorStoreConfig::default(),
            in_memory: false,
        }
    }
}

impl SystemConfig {
    /// 创建新的系统配置
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置字典库路径
    pub fn with_dictionary_path(mut self, path: impl Into<String>) -> Self {
        self.dictionary_path = path.into();
        self
    }

    /// 设置关系库路径
    pub fn with_relational_path(mut self, path: impl Into<String>) -> Self {
        self.relational_path = path.into();
        self
    }

    /// 设置向量库配置
    pub fn with_vector_config(mut self, config: VectorStoreConfig) -> Self {
        self.vector_config = config;
        self
    }

    /// 设置向量库路径
    pub fn with_vector_path(mut self, path: impl Into<String>) -> Self {
        self.vector_config.path = path.into();
        self
    }

    /// 设置向量维度
    pub fn with_vector_dimension(mut self, dimension: usize) -> Self {
        self.vector_config.dimension = dimension;
        self
    }

    /// 设置为内存模式
    pub fn in_memory(mut self) -> Self {
        self.in_memory = true;
        self
    }
}

/// 系统初始化器
pub struct SystemInitializer;

impl SystemInitializer {
    /// 初始化系统（完整模式）
    ///
    /// # 参数
    /// - `config`: 系统配置
    /// - `llm_client`: LLM 客户端
    /// - `embedder`: 嵌入生成器
    /// - `extractor`: 记忆提取器
    ///
    /// # 返回
    /// 三库记忆管理器实例
    pub async fn initialize(
        config: SystemConfig,
        llm_client: Arc<dyn LlmClient>,
        embedder: Arc<dyn Embedder>,
        extractor: Arc<dyn MemoryExtractor>,
    ) -> Result<ThreeLibraryMemoryManager, ShouxinError> {
        let (dictionary_store, relational_store, vector_store, embedded_vector_store) =
            Self::initialize_stores(&config).await?;

        let dispatcher = DataDispatcher::new(
            dictionary_store.clone(),
            relational_store.clone(),
            vector_store.clone(),
        )
        .with_embedder(embedder.clone());

        Ok(ThreeLibraryMemoryManager::with_embedded_vector_store(
            dictionary_store,
            relational_store,
            vector_store,
            embedded_vector_store,
            llm_client,
            embedder,
            extractor,
            Arc::new(dispatcher),
        ))
    }

    /// 初始化存储层
    ///
    /// # 参数
    /// - `config`: 系统配置
    ///
    /// # 返回
    /// (字典库, 关系库, 向量库 trait, 嵌入式向量库)
    pub async fn initialize_stores(
        config: &SystemConfig,
    ) -> Result<
        (
            Arc<DictionaryStore>,
            Arc<RelationalStore>,
            Arc<dyn VectorStore>,
            Arc<AnyVectorStore>,
        ),
        ShouxinError,
    > {
        let dictionary_store = if config.in_memory {
            Arc::new(DictionaryStore::in_memory()?)
        } else {
            Arc::new(DictionaryStore::open(&config.dictionary_path)?)
        };

        let relational_store = if config.in_memory {
            Arc::new(RelationalStore::in_memory().await?)
        } else {
            Arc::new(RelationalStore::open(&config.relational_path).await?)
        };

        let embedded_vector_store = if config.vector_config.store_type == VectorStoreType::Hnsw {
            let store = Arc::new(HnswVectorStore::new_with_dimension(
                &config.vector_config.path,
                config.vector_config.dimension,
            )?);
            store.load_existing().await?;
            AnyVectorStore::Hnsw(store)
        } else {
            let store = if config.in_memory {
                Arc::new(EmbeddedVectorStore::in_memory(
                    config.vector_config.dimension,
                )?)
            } else {
                Arc::new(EmbeddedVectorStore::new_with_dimension(
                    &config.vector_config.path,
                    config.vector_config.dimension,
                )?)
            };
            AnyVectorStore::Embedded(store)
        };

        let embedded_vector_store: Arc<AnyVectorStore> = Arc::new(embedded_vector_store);

        let vector_store: Arc<dyn VectorStore> = embedded_vector_store.clone() as Arc<dyn VectorStore>;

        Ok((
            dictionary_store,
            relational_store,
            vector_store,
            embedded_vector_store,
        ))
    }

    /// 使用工厂模式初始化向量库
    ///
    /// # 参数
    /// - `config`: 向量库配置
    ///
    /// # 返回
    /// 向量库实例
    pub fn initialize_vector_store(
        config: &VectorStoreConfig,
    ) -> Result<Arc<dyn VectorStore>, ShouxinError> {
        VectorStoreFactory::create(config)
    }

    /// 初始化最小系统（用于测试）
    ///
    /// # 返回
    /// 三库记忆管理器实例（使用占位符组件）
    pub async fn initialize_minimal() -> Result<ThreeLibraryMemoryManager, ShouxinError> {
        let dictionary_store = Arc::new(DictionaryStore::in_memory()?);
        let relational_store = Arc::new(RelationalStore::in_memory().await?);
        let vector_store = Arc::new(EmbeddedVectorStore::in_memory(768)?);
        let embedded_vector_store: Arc<AnyVectorStore> = Arc::new(AnyVectorStore::Embedded(vector_store.clone()));

        let dispatcher = DataDispatcher::new(
            dictionary_store.clone(),
            relational_store.clone(),
            vector_store.clone(),
        );

        struct MockLlmClient;
        #[async_trait::async_trait]
        impl LlmClient for MockLlmClient {
            async fn chat(
                &self,
                _messages: Vec<crate::core::Message>,
            ) -> Result<String, ShouxinError> {
                Ok("mock response".to_string())
            }

            async fn extract_facts(
                &self,
                _conversation: &str,
            ) -> Result<Vec<crate::core::Fact>, ShouxinError> {
                Ok(Vec::new())
            }

            async fn decide_memory_action(
                &self,
                _new_fact: &crate::core::Fact,
                _existing_memories: Vec<&crate::core::Memory>,
            ) -> Result<crate::core::MemoryAction, ShouxinError> {
                Ok(crate::core::MemoryAction::Add)
            }

            fn model_name(&self) -> &str {
                "mock"
            }

            async fn complete(&self, _prompt: &str) -> Result<String, ShouxinError> {
                Ok("mock completion".to_string())
            }
        }

        struct MockEmbedder;
        #[async_trait::async_trait]
        impl Embedder for MockEmbedder {
            async fn embed(&self, _text: &str) -> Result<crate::core::Embedding, ShouxinError> {
                crate::core::Embedding::new(vec![0.0; 768], "mock".to_string())
                    .map_err(|e| ShouxinError::EmbeddingError(e.to_string()))
            }

            async fn embed_batch(
                &self,
                texts: Vec<String>,
            ) -> Result<Vec<crate::core::Embedding>, ShouxinError> {
                let mut embeddings = Vec::new();
                for _ in texts {
                    embeddings.push(
                        crate::core::Embedding::new(vec![0.0; 768], "mock".to_string())
                            .map_err(|e| ShouxinError::EmbeddingError(e.to_string()))?,
                    );
                }
                Ok(embeddings)
            }

            fn dimension(&self) -> usize {
                768
            }

            fn model_name(&self) -> &str {
                "mock"
            }
        }

        struct MockExtractor;
        #[async_trait::async_trait]
        impl MemoryExtractor for MockExtractor {
            async fn extract(
                &self,
                _conversation: &[crate::core::Message],
            ) -> Result<crate::memory_extractor::ExtractedElements, ShouxinError> {
                Ok(crate::memory_extractor::ExtractedElements::new(
                    String::new(),
                ))
            }
            fn fallback_extract(
                &self,
                _conversation: &str,
            ) -> Result<crate::memory_extractor::ExtractedElements, ShouxinError> {
                Ok(crate::memory_extractor::ExtractedElements::new(
                    String::new(),
                ))
            }
        }

        Ok(ThreeLibraryMemoryManager::with_embedded_vector_store(
            dictionary_store,
            relational_store,
            vector_store,
            embedded_vector_store,
            Arc::new(MockLlmClient),
            Arc::new(MockEmbedder),
            Arc::new(MockExtractor),
            Arc::new(dispatcher),
        ))
    }
}

/// 初始化助手
pub struct InitializationHelper;

impl InitializationHelper {
    /// 创建默认数据目录
    pub fn ensure_data_dirs(config: &SystemConfig) -> Result<(), ShouxinError> {
        if !config.in_memory {
            std::fs::create_dir_all(&config.dictionary_path).map_err(|e| {
                ShouxinError::StorageError(format!("Failed to create dictionary directory: {}", e))
            })?;

            let relational_dir = std::path::Path::new(&config.relational_path)
                .parent()
                .ok_or_else(|| ShouxinError::StorageError("Invalid relational path".to_string()))?;
            std::fs::create_dir_all(relational_dir).map_err(|e| {
                ShouxinError::StorageError(format!("Failed to create relational directory: {}", e))
            })?;

            std::fs::create_dir_all(&config.vector_config.path).map_err(|e| {
                ShouxinError::StorageError(format!("Failed to create vector directory: {}", e))
            })?;
        }
        Ok(())
    }

    /// 验证配置
    pub fn validate_config(config: &SystemConfig) -> Result<(), ShouxinError> {
        if config.vector_config.dimension == 0 {
            return Err(ShouxinError::ConfigError(
                "Vector dimension must be greater than 0".to_string(),
            ));
        }

        if !config.in_memory {
            if config.dictionary_path.is_empty() {
                return Err(ShouxinError::ConfigError(
                    "Dictionary path cannot be empty".to_string(),
                ));
            }
            if config.relational_path.is_empty() {
                return Err(ShouxinError::ConfigError(
                    "Relational path cannot be empty".to_string(),
                ));
            }
            if config.vector_config.path.is_empty() {
                return Err(ShouxinError::ConfigError(
                    "Vector store path cannot be empty".to_string(),
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_config_default() {
        let config = SystemConfig::default();
        assert_eq!(config.dictionary_path, "./data/dictionary");
        assert_eq!(config.vector_config.dimension, 768);
        assert!(!config.in_memory);
    }

    #[test]
    fn test_system_config_builder() {
        let config = SystemConfig::new()
            .with_dictionary_path("/custom/dict")
            .with_vector_dimension(512)
            .in_memory();

        assert_eq!(config.dictionary_path, "/custom/dict");
        assert_eq!(config.vector_config.dimension, 512);
        assert!(config.in_memory);
    }

    #[test]
    fn test_validate_config() {
        let valid_config = SystemConfig::default();
        assert!(InitializationHelper::validate_config(&valid_config).is_ok());

        let invalid_config = SystemConfig {
            vector_config: VectorStoreConfig {
                dimension: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(InitializationHelper::validate_config(&invalid_config).is_err());
    }

    #[tokio::test]
    async fn test_initialize_minimal() {
        let result = SystemInitializer::initialize_minimal().await;
        assert!(result.is_ok());
    }
}
