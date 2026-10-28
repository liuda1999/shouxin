//! 向量库存储实现
//!
//! 使用 sled 作为元数据和向量存储，提供线性搜索（兼容 HNSW 的简单实现）

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::core::{Embedding, Memory, MemoryFilter, SearchResult, SearchSource};
use crate::error::ShouxinError;
use crate::stores::vector::types::{LearningMaterial, VectorSearchResult};
use crate::traits::vector_store::{DistanceMetric, VectorStore, VectorStoreConfig};

/// 存储记录（包含记忆和向量）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StoredRecord {
    memory: Memory,
    pub(crate) vector: Vec<f32>,
    pub(crate) material: Option<LearningMaterial>,
}

/// 嵌入式向量库存储实现
pub struct EmbeddedVectorStore {
    /// sled 数据库
    db: sled::Db,
    /// 向量维度
    dimension: usize,
    /// 内存中的向量缓存（用于快速搜索）
    pub(crate) cache: RwLock<HashMap<String, StoredRecord>>,
}

impl EmbeddedVectorStore {
    /// 创建新的嵌入式向量库存储
    pub fn new(path: &str) -> Result<Self, ShouxinError> {
        Self::new_with_dimension(path, 768)
    }

    /// 创建新的嵌入式向量库存储（指定维度）
    pub fn new_with_dimension(path: &str, dimension: usize) -> Result<Self, ShouxinError> {
        let db = sled::open(path)?;

        let mut cache = HashMap::new();

        for item in db.iter() {
            let (k, v) = item?;
            let key = String::from_utf8_lossy(&k).to_string();
            let record: StoredRecord = serde_json::from_slice(&v)?;
            cache.insert(key, record);
        }

        println!(
            "向量存储初始化完成，使用 {} 维向量，现有 {} 条记录",
            dimension,
            cache.len()
        );

        Ok(Self {
            db,
            dimension,
            cache: RwLock::new(cache),
        })
    }

    /// 创建新的嵌入式向量库存储（不加载嵌入模型）
    pub fn new_without_embedder(path: &str, dimension: usize) -> Result<Self, ShouxinError> {
        Self::new_with_dimension(path, dimension)
    }

    /// 创建内存模式的向量库（用于测试，不加载嵌入模型）
    pub fn in_memory(dimension: usize) -> Result<Self, ShouxinError> {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join(format!("shouxin_vector_{}", uuid::Uuid::new_v4()));
        let path_str = path.to_string_lossy().to_string();
        Self::new_without_embedder(&path_str, dimension)
    }

    /// 创建内存模式的向量库（用于测试）
    pub fn new_in_memory() -> Result<Self, ShouxinError> {
        Self::in_memory(768)
    }

    /// 计算余弦相似度
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let mut dot_product = 0.0;
        let mut norm_a = 0.0;
        let mut norm_b = 0.0;

        for i in 0..a.len() {
            dot_product += a[i] * b[i];
            norm_a += a[i] * a[i];
            norm_b += b[i] * b[i];
        }

        let norm_a = norm_a.sqrt();
        let norm_b = norm_b.sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot_product / (norm_a * norm_b)
        }
    }

    /// 保存记录到 sled
    fn save_record(&self, id: &str, record: &StoredRecord) -> Result<(), ShouxinError> {
        let bytes = serde_json::to_vec(record)?;
        self.db.insert(id.as_bytes(), bytes)?;
        self.db.flush()?;
        Ok(())
    }

    /// 生成文本的嵌入向量（占位实现）
    pub fn embed(&self, _text: &str) -> Result<Embedding, ShouxinError> {
        Err(ShouxinError::GeneralError(
            "Embedder not initialized, please provide pre-computed embeddings".to_string(),
        ))
    }

    /// 插入学习素材（返回 ID）
    pub async fn insert_material(
        &self,
        content: &str,
        summary: Option<&str>,
        source_id: Option<&str>,
        material_type: &str,
    ) -> Result<String, ShouxinError> {
        let embedding = self.embed(content)?;
        self.insert_material_with_embedding(content, summary, source_id, material_type, &embedding, crate::core::contact::OwnerType::User, "default_user")
            .await
    }

    /// 插入学习素材（使用预计算的嵌入）
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_material_with_embedding(
        &self,
        content: &str,
        summary: Option<&str>,
        source_id: Option<&str>,
        material_type: &str,
        embedding: &Embedding,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<String, ShouxinError> {
        let id = uuid::Uuid::new_v4().to_string();

        let mut material = LearningMaterial::new_with_owner(
            0, 
            content.to_string(), 
            material_type.to_string(),
            owner_type,
            owner_id.to_string(),
        );

        if let Some(s) = summary {
            material = material.with_summary(s.to_string());
        }

        if let Some(sid) = source_id {
            material = material.with_source_conversation_id(sid.to_string());
        }

        let mut memory = Memory::new(content.to_string())?;
        memory.id = id.clone();
        memory.embedding = Some(embedding.clone());

        let record = StoredRecord {
            memory,
            vector: embedding.vector.clone(),
            material: Some(material),
        };

        {
            let mut cache = self.cache.write().await;
            cache.insert(id.clone(), record.clone());
        }

        self.save_record(&id, &record)?;

        Ok(id)
    }

    /// 语义搜索（接收文本查询）
    pub async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        owner_type: Option<crate::core::contact::OwnerType>,
        owner_id: Option<&str>,
    ) -> Result<Vec<VectorSearchResult>, ShouxinError> {
        let query_embedding = self.embed(query)?;
        self.semantic_search_with_embedding(&query_embedding, limit, owner_type, owner_id)
            .await
    }

    /// 语义搜索（接收嵌入向量）
    pub async fn semantic_search_with_embedding(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        owner_type: Option<crate::core::contact::OwnerType>,
        owner_id: Option<&str>,
    ) -> Result<Vec<VectorSearchResult>, ShouxinError> {
        let cache = self.cache.read().await;
        let mut results = Vec::new();

        for (_, record) in cache.iter() {
            if let Some(material) = &record.material {
                let matches_owner = match (&owner_type, owner_id) {
                    (Some(ot), Some(oid)) => material.owner_type == *ot && material.owner_id == oid,
                    (Some(ot), None) => material.owner_type == *ot,
                    (None, Some(oid)) => material.owner_id == oid,
                    (None, None) => true,
                };

                if matches_owner {
                    let similarity = Self::cosine_similarity(&query_embedding.vector, &record.vector);
                    results.push((similarity, material.clone()));
                }
            } else {
                let mem = &record.memory;
                let mem_owner = mem.metadata
                    .get("owner_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user");
                let mem_owner_id = mem.metadata
                    .get("owner_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default_user")
                    .to_string();
                let material_type = mem
                    .metadata
                    .get("material_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("file_chunk")
                    .to_string();

                let matches_owner = match (&owner_type, owner_id) {
                    (Some(ot), Some(oid)) => {
                        let ot_str = format!("{:?}", ot).to_lowercase();
                        mem_owner.to_lowercase() == ot_str && mem_owner_id == oid
                    }
                    (Some(ot), None) => {
                        let ot_str = format!("{:?}", ot).to_lowercase();
                        mem_owner.to_lowercase() == ot_str
                    }
                    (None, Some(oid)) => mem_owner_id == oid,
                    (None, None) => true,
                };

                if matches_owner {
                    let similarity = Self::cosine_similarity(&query_embedding.vector, &record.vector);
                    let mat = LearningMaterial {
                        id: 0,
                        content: mem.content.clone(),
                        summary: None,
                        source_conversation_id: mem.metadata
                            .get("source_conversation_id")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        created_at: Utc::now(),
                        material_type,
                        canonical_entity_id: None,
                        keywords: Vec::new(),
                        context_before_id: None,
                        context_after_id: None,
                        triples: Vec::new(),
                        owner_type: match mem_owner {
                            "agent" => crate::core::contact::OwnerType::Agent,
                            _ => crate::core::contact::OwnerType::User,
                        },
                        owner_id: mem_owner_id,
                    };
                    results.push((similarity, mat));
                }
            }
        }

        results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        if limit > 0 && results.len() > limit {
            results.truncate(limit);
        }

        Ok(results
            .into_iter()
            .map(|(score, material)| VectorSearchResult::new(material, score))
            .collect())
    }

    /// 获取学习素材
    pub async fn get_material(&self, id: &str) -> Result<Option<LearningMaterial>, ShouxinError> {
        let cache = self.cache.read().await;
        Ok(cache.get(id).and_then(|r| r.material.clone()))
    }

    /// 删除学习素材
    pub async fn delete_material(&self, id: &str) -> Result<(), ShouxinError> {
        {
            let mut cache = self.cache.write().await;
            cache.remove(id);
        }
        self.db.remove(id.as_bytes())?;
        Ok(())
    }

    /// 获取素材数量
    pub async fn count_materials(&self) -> Result<usize, ShouxinError> {
        let cache = self.cache.read().await;
        // R10：只统计"技术类学习素材/经验数据集"，不含事件、文件分块、文章诗歌等
        Ok(cache
            .values()
            .filter(|r| {
                r.material
                    .as_ref()
                    .map(|m| crate::stores::vector::types::is_countable_material_type(&m.material_type))
                    .unwrap_or(false)
            })
            .count())
    }

    /// 列出所有学习素材
    pub async fn list_materials(
        &self,
        limit: Option<usize>,
        owner_type: Option<crate::core::contact::OwnerType>,
        owner_id: Option<&str>,
    ) -> Result<Vec<LearningMaterial>, ShouxinError> {
        let cache = self.cache.read().await;
        let mut materials = Vec::new();
        let limit = limit.unwrap_or(usize::MAX);

        for (_, record) in cache.iter() {
            if let Some(material) = &record.material {
                // R10：只列出"技术类学习素材/经验数据集"
                if !crate::stores::vector::types::is_countable_material_type(&material.material_type) {
                    continue;
                }
                // 应用 owner 过滤
                let matches_owner = match (&owner_type, owner_id) {
                    (Some(ot), Some(oid)) => material.owner_type == *ot && material.owner_id == oid,
                    (Some(ot), None) => material.owner_type == *ot,
                    (None, Some(oid)) => material.owner_id == oid,
                    (None, None) => true,
                };
                
                if matches_owner {
                    materials.push(material.clone());
                    if materials.len() >= limit {
                        break;
                    }
                }
            }
        }

        Ok(materials)
    }

    /// 清空所有学习素材
    pub async fn clear_materials(&self) -> Result<(), ShouxinError> {
        {
            let mut cache = self.cache.write().await;
            cache.clear();
        }
        self.db.clear()?;
        Ok(())
    }

    /// 显式保存到磁盘
    pub fn flush(&self) -> Result<(), ShouxinError> {
        self.db.flush()?;
        Ok(())
    }
}

#[async_trait]
impl VectorStore for EmbeddedVectorStore {
    async fn insert(&self, memory: Memory) -> Result<(), ShouxinError> {
        let embedding = memory
            .embedding
            .as_ref()
            .ok_or_else(|| ShouxinError::InvalidArgument("Memory must have embedding".to_string()))?;

        if embedding.vector.len() != self.dimension {
            return Err(ShouxinError::InvalidArgument(format!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.vector.len()
            )));
        }

        let record = StoredRecord {
            memory: memory.clone(),
            vector: embedding.vector.clone(),
            material: None,
        };

        {
            let mut cache = self.cache.write().await;
            cache.insert(memory.id.clone(), record.clone());
        }

        self.save_record(&memory.id, &record)?;

        Ok(())
    }

    async fn search(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        _filters: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<Vec<SearchResult>, ShouxinError> {
        if query_embedding.vector.len() != self.dimension {
            return Err(ShouxinError::InvalidArgument(format!(
                "Query embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                query_embedding.vector.len()
            )));
        }

        let cache = self.cache.read().await;
        let mut results = Vec::new();

        for (_, record) in cache.iter() {
            let similarity = Self::cosine_similarity(&query_embedding.vector, &record.vector);
            results.push((similarity, record.memory.clone()));
        }

        results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        if limit > 0 && results.len() > limit {
            results.truncate(limit);
        }

        Ok(results
            .into_iter()
            .map(|(score, memory)| SearchResult {
                memory,
                score,
                context: None,
                matched_text: None,
                source: SearchSource::Vector,
            })
            .collect())
    }

    async fn search_with_filter(
        &self,
        query_embedding: &Embedding,
        _filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<SearchResult>, ShouxinError> {
        self.search(query_embedding, limit, None).await
    }

    async fn delete(&self, memory_id: &str) -> Result<(), ShouxinError> {
        {
            let mut cache = self.cache.write().await;
            cache.remove(memory_id);
        }
        self.db.remove(memory_id.as_bytes())?;
        Ok(())
    }

    async fn update(&self, memory: Memory) -> Result<(), ShouxinError> {
        self.delete(&memory.id).await?;
        self.insert(memory).await
    }

    async fn get(&self, memory_id: &str) -> Result<Option<Memory>, ShouxinError> {
        let cache = self.cache.read().await;
        Ok(cache.get(memory_id).map(|r| r.memory.clone()))
    }

    async fn list(&self, _filter: Option<&MemoryFilter>) -> Result<Vec<Memory>, ShouxinError> {
        let cache = self.cache.read().await;
        Ok(cache.values().map(|r| r.memory.clone()).collect())
    }

    async fn count(&self, _filter: Option<&MemoryFilter>) -> Result<usize, ShouxinError> {
        let cache = self.cache.read().await;
        Ok(cache.len())
    }

    async fn clear(&self) -> Result<(), ShouxinError> {
        self.clear_materials().await
    }
}

/// 向量库构建器
pub struct VectorStoreBuilder {
    config: VectorStoreConfig,
}

impl VectorStoreBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self {
            config: VectorStoreConfig::default(),
        }
    }

    /// 设置存储路径（持久化模式）
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.config.path = Some(path.into());
        self.config.in_memory = false;
        self
    }

    /// 设置为内存模式
    pub fn in_memory(mut self) -> Self {
        self.config.in_memory = true;
        self
    }

    /// 设置嵌入维度
    pub fn with_dimension(mut self, dimension: usize) -> Self {
        self.config.dimension = dimension;
        self
    }

    /// 设置距离度量类型
    pub fn with_distance_metric(mut self, metric: DistanceMetric) -> Self {
        self.config.distance_metric = metric;
        self
    }

    /// 构建向量库存储实例（不加载嵌入模型）
    pub fn build_without_embedder(
        self,
        dimension: usize,
    ) -> Result<EmbeddedVectorStore, ShouxinError> {
        if self.config.in_memory {
            EmbeddedVectorStore::in_memory(dimension)
        } else {
            let path = self
                .config
                .path
                .unwrap_or("./data/vector_store".to_string());
            EmbeddedVectorStore::new_without_embedder(&path, dimension)
        }
    }

    /// 构建向量库存储实例
    pub fn build(self) -> Result<EmbeddedVectorStore, ShouxinError> {
        if self.config.in_memory {
            EmbeddedVectorStore::new_in_memory()
        } else {
            let path = self
                .config
                .path
                .unwrap_or("./data/vector_store".to_string());
            EmbeddedVectorStore::new(&path)
        }
    }
}

impl Default for VectorStoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}
