//! 真实 HNSW 向量库存储实现
//!
//! 使用 hnsw_rs 库的 HNSW 索引实现近似最近邻搜索，
//! 替代之前的暴力搜索（线性扫描）。

use async_trait::async_trait;
use hnsw_rs::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sled::{Config, Mode};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;

use crate::core::{Embedding, Memory, MemoryFilter, SearchResult};
use crate::core::contact::OwnerType;
use crate::error::ShouxinError;
use crate::stores::vector::types::{LearningMaterial, VectorIndexConfig, VectorSearchResult};
use crate::traits::vector_store::VectorStore;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StoredRecord {
    pub memory: Memory,
    pub(crate) vector: Vec<f32>,
    pub(crate) material: Option<LearningMaterial>,
}

/// 把 `Memory` 记录还原为 `LearningMaterial`
///
/// 异步提取路径通过 `VectorStore::insert(Memory)` 写入（owner 存在 metadata），
/// 同步/文件路径通过 `insert_material_with_embedding` 写入（owner 存在素材字段）。
/// 为了让素材的**检索、详情、计数、列表**对两种写入方式一视同仁，
/// 统一在这里做一次转换，避免各处各写一遍（曾导致 HNSW 下异步素材完全查不到）。
fn memory_to_material(mem: &crate::core::Memory) -> LearningMaterial {
    let mem_owner = mem
        .metadata
        .get("owner_type")
        .and_then(|v| v.as_str())
        .unwrap_or("user");
    let owner_type = crate::core::contact::OwnerType::parse(mem_owner)
        .unwrap_or(crate::core::contact::OwnerType::User);
    let owner_id = mem
        .metadata
        .get("owner_id")
        .and_then(|v| v.as_str())
        .unwrap_or(crate::core::owner::DEFAULT_USER_ID)
        .to_string();
    let material_type = mem
        .metadata
        .get("material_type")
        .and_then(|v| v.as_str())
        .unwrap_or("file_chunk")
        .to_string();

    LearningMaterial {
        id: 0,
        content: mem.content.clone(),
        summary: None,
        source_conversation_id: mem
            .metadata
            .get("source_conversation_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        created_at: mem.created_at,
        material_type,
        canonical_entity_id: None,
        keywords: Vec::new(),
        context_before_id: None,
        context_after_id: None,
        triples: Vec::new(),
        owner_type,
        owner_id,
    }
}

/// 该 `Memory` 是否代表"学习素材"（口径见 `types::COUNTED_MATERIAL_TYPES`）
fn is_learning_memory(mem: &crate::core::Memory) -> bool {
    mem.metadata
        .get("material_type")
        .and_then(|v| v.as_str())
        .map(crate::stores::vector::types::is_countable_material_type)
        .unwrap_or(false)
}

pub struct HnswVectorStore {
    hnsw: RwLock<Hnsw<'static, f32, DistCosine>>,
    records: RwLock<HashMap<usize, StoredRecord>>,
    id_to_uuid: RwLock<HashMap<usize, String>>,
    uuid_to_id: RwLock<HashMap<String, usize>>,
    id_counter: AtomicUsize,
    dimension: usize,
    config: VectorIndexConfig,
    db: sled::Db,
}

impl HnswVectorStore {
    pub fn new(path: &str, dimension: usize, config: VectorIndexConfig) -> Result<Self, ShouxinError> {
        let db = Config::new()
            .path(path)
            .mode(Mode::LowSpace)
            // 每 5 秒落盘一次（原 1 秒）：降低写放大，关闭时另有显式 flush 兜底
            .flush_every_ms(Some(5000))
            .open()
            .map_err(|e| ShouxinError::StorageError(format!("Failed to open sled: {}", e)))?;

        let max_elements = 10000;
        let max_layer = 16;

        let hnsw = Hnsw::new(
            config.m,
            max_elements,
            max_layer,
            config.ef_construction,
            DistCosine,
        );

        let store = Self {
            hnsw: RwLock::new(hnsw),
            records: RwLock::new(HashMap::new()),
            id_to_uuid: RwLock::new(HashMap::new()),
            uuid_to_id: RwLock::new(HashMap::new()),
            id_counter: AtomicUsize::new(0),
            dimension,
            config,
            db,
        };

        Ok(store)
    }

    pub fn new_with_dimension(path: &str, dimension: usize) -> Result<Self, ShouxinError> {
        let config = VectorIndexConfig::default().with_dimension(dimension);
        Self::new(path, dimension, config)
    }

    pub fn in_memory(dimension: usize) -> Result<Self, ShouxinError> {
        let config = VectorIndexConfig::default().with_dimension(dimension);
        let db = sled::Config::new()
            .temporary(true)
            .open()
            .map_err(|e| ShouxinError::StorageError(format!("Failed to open sled: {}", e)))?;

        let max_elements = 10000;
        let max_layer = 16;

        let hnsw = Hnsw::new(
            config.m,
            max_elements,
            max_layer,
            config.ef_construction,
            DistCosine,
        );

        Ok(Self {
            hnsw: RwLock::new(hnsw),
            records: RwLock::new(HashMap::new()),
            id_to_uuid: RwLock::new(HashMap::new()),
            uuid_to_id: RwLock::new(HashMap::new()),
            id_counter: AtomicUsize::new(0),
            dimension,
            config,
            db,
        })
    }

    pub async fn load_existing(&self) -> Result<usize, ShouxinError> {
        let mut count = 0;
        let hnsw = self.hnsw.write().await;
        let mut records = self.records.write().await;
        let mut id_to_uuid = self.id_to_uuid.write().await;
        let mut uuid_to_id = self.uuid_to_id.write().await;

        let mut max_id: usize = 0;
        let mut pending: Vec<(usize, String, Vec<f32>, StoredRecord)> = Vec::new();

        for item in self.db.iter() {
            let (k, v) = item.map_err(|e| ShouxinError::StorageError(format!("sled iter error: {}", e)))?;
            let key = String::from_utf8_lossy(&k).to_string();
            if key.starts_with("__") {
                continue;
            }
            let record: StoredRecord = serde_json::from_slice(&v)
                .map_err(|e| ShouxinError::DeserializationError(format!("deserialize error: {}", e)))?;

            if record.vector.is_empty() {
                continue;
            }

            let point_id = max_id + 1;
            max_id = point_id;
            pending.push((point_id, key.clone(), record.vector.clone(), record));
            count += 1;
        }

        for (point_id, uuid, vector, record) in pending {
            hnsw.insert_slice((&vector, point_id));
            id_to_uuid.insert(point_id, uuid.clone());
            uuid_to_id.insert(uuid, point_id);
            records.insert(point_id, record);
        }

        self.id_counter.store(max_id + 1, Ordering::SeqCst);

        tracing::info!("HNSW loaded {} existing records from sled", count);
        Ok(count)
    }

    fn next_id(&self) -> usize {
        self.id_counter.fetch_add(1, Ordering::SeqCst)
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let mut dot = 0.0f64;
        let mut na = 0.0f64;
        let mut nb = 0.0f64;
        for i in 0..a.len().min(b.len()) {
            dot += (a[i] as f64) * (b[i] as f64);
            na += (a[i] as f64) * (a[i] as f64);
            nb += (b[i] as f64) * (b[i] as f64);
        }
        let denom = na.sqrt() * nb.sqrt();
        if denom < 1e-12 {
            0.0
        } else {
            (dot / denom) as f32
        }
    }

    fn save_record(&self, uuid: &str, record: &StoredRecord) -> Result<(), ShouxinError> {
        let data = serde_json::to_vec(record)
            .map_err(|e| ShouxinError::SerializationError(format!("serialize error: {}", e)))?;
        self.db
            .insert(uuid.as_bytes(), data)
            .map_err(|e| ShouxinError::StorageError(format!("sled insert error: {}", e)))?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_material_with_embedding(
        &self,
        content: &str,
        summary: Option<&str>,
        source_id: Option<&str>,
        material_type: &str,
        embedding: &Embedding,
        owner_type: OwnerType,
        owner_id: &str,
    ) -> Result<String, ShouxinError> {
        if embedding.vector.len() != self.dimension {
            return Err(ShouxinError::InvalidArgument(format!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.vector.len()
            )));
        }

        let uuid = uuid::Uuid::new_v4().to_string();
        let point_id = self.next_id();

        let mut material = LearningMaterial::new_with_owner(
            point_id as u64,
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

        let mut memory = Memory::new(content.to_string()).map_err(|e| ShouxinError::GeneralError(format!("Memory creation failed: {}", e)))?;
        memory.id = uuid.clone();
        memory.embedding = Some(embedding.clone());

        let record = StoredRecord {
            memory,
            vector: embedding.vector.clone(),
            material: Some(material),
        };

        {
            let hnsw = self.hnsw.write().await;
            hnsw.insert_slice((&embedding.vector, point_id));
        }
        {
            let mut records = self.records.write().await;
            records.insert(point_id, record.clone());
        }
        {
            let mut id_to_uuid = self.id_to_uuid.write().await;
            let mut uuid_to_id = self.uuid_to_id.write().await;
            id_to_uuid.insert(point_id, uuid.clone());
            uuid_to_id.insert(uuid.clone(), point_id);
        }

        self.save_record(&uuid, &record)?;

        Ok(uuid)
    }

    pub async fn semantic_search_with_embedding(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        owner_type: Option<OwnerType>,
        owner_id: Option<&str>,
    ) -> Result<Vec<VectorSearchResult>, ShouxinError> {
        let ef = self.config.ef_search;
        let knbn = limit * 2;

        let neighbours = {
            let hnsw = self.hnsw.read().await;
            hnsw.search(&query_embedding.vector, knbn, ef)
        };

        let records = self.records.read().await;
        let mut results = Vec::new();

        for n in &neighbours {
            // records 存在性兜底过滤：HNSW 索引可能返回已被删除的 id，
            // 若该 id 在 records 中不存在（内存索引已删除），跳过该结果。
            let record = match records.get(&n.d_id) {
                Some(r) => r,
                None => {
                    tracing::debug!(
                        point_id = n.d_id,
                        "search skip: point_id not found in records (possibly deleted)"
                    );
                    continue;
                }
            };
            if let Some(material) = &record.material {
                let matches_owner = match (&owner_type, owner_id) {
                    (Some(ot), Some(oid)) => material.owner_type == *ot && material.owner_id == oid,
                    (Some(ot), None) => material.owner_type == *ot,
                    (None, Some(oid)) => material.owner_id == oid,
                    (None, None) => true,
                };

                if matches_owner {
                    let similarity = Self::cosine_similarity(&query_embedding.vector, &record.vector);
                    results.push(VectorSearchResult::new(material.clone(), similarity));
                }
            } else {
                // R1 修复：vector 通过 `VectorStore::insert(Memory)` 写入的记录（异步提取路径）
                // 没有 material 字段，旧实现直接跳过 → 这些学习素材在素材检索里"查不到"。
                // 这里按 memory.metadata 还原成 LearningMaterial，与内嵌向量库行为对齐。
                let mem = &record.memory;
                let mem_owner_type = crate::core::contact::OwnerType::parse(
                    mem.metadata
                        .get("owner_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("user"),
                )
                .unwrap_or(crate::core::contact::OwnerType::User);
                let mem_owner_id = mem
                    .metadata
                    .get("owner_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(crate::core::owner::DEFAULT_USER_ID)
                    .to_string();

                let matches_owner = match (&owner_type, owner_id) {
                    (Some(ot), Some(oid)) => mem_owner_type == *ot && mem_owner_id == oid,
                    (Some(ot), None) => mem_owner_type == *ot,
                    (None, Some(oid)) => mem_owner_id == oid,
                    (None, None) => true,
                };

                if matches_owner {
                    let similarity = Self::cosine_similarity(&query_embedding.vector, &record.vector);
                    let material = memory_to_material(mem);
                    results.push(VectorSearchResult::new(material, similarity));
                }
            }
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

    pub async fn get_material(&self, id: &str) -> Result<Option<LearningMaterial>, ShouxinError> {
        let uuid_to_id = self.uuid_to_id.read().await;
        if let Some(&point_id) = uuid_to_id.get(id) {
            let records = self.records.read().await;
            Ok(records.get(&point_id).and_then(|r| {
                // material 记录直接返回；memory 记录（异步提取路径写入）按 metadata 还原，
                // 否则这些素材的详情接口会 404。
                r.material.clone().or_else(|| {
                    is_learning_memory(&r.memory).then(|| memory_to_material(&r.memory))
                })
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn delete_material(&self, id: &str) -> Result<(), ShouxinError> {
        // 先删除 sled（持久化层）：sled 是持久化层，records 是内存索引。
        // 若 sled 删除失败则直接返回错误，records 保持不变，下次加载时状态一致。
        // 若 sled 删除成功但后续 records 删除失败（内存操作不会失败），
        // 下次加载时 records 不会重建已删的条目，保证最终一致性。
        self.db
            .remove(id.as_bytes())
            .map_err(|e| ShouxinError::StorageError(format!("sled remove error: {}", e)))?;

        // 再删除 records（内存索引）和映射表
        let uuid_to_id = self.uuid_to_id.read().await;
        if let Some(&point_id) = uuid_to_id.get(id) {
            drop(uuid_to_id);
            let mut records = self.records.write().await;
            records.remove(&point_id);
            let mut id_to_uuid = self.id_to_uuid.write().await;
            let mut uuid_to_id_w = self.uuid_to_id.write().await;
            id_to_uuid.remove(&point_id);
            uuid_to_id_w.remove(id);
        } else {
            drop(uuid_to_id);
        }
        Ok(())
    }

    pub async fn count_materials(&self) -> Result<usize, ShouxinError> {
        let records = self.records.read().await;
        // R10：只统计"技术类学习素材/经验数据集"（含 material 与 memory 两种写入形态）
        Ok(records
            .values()
            .filter(|r| match &r.material {
                Some(m) => crate::stores::vector::types::is_countable_material_type(&m.material_type),
                None => is_learning_memory(&r.memory),
            })
            .count())
    }

    pub async fn list_materials(
        &self,
        limit: Option<usize>,
        owner_type: Option<OwnerType>,
        owner_id: Option<&str>,
    ) -> Result<Vec<LearningMaterial>, ShouxinError> {
        let records = self.records.read().await;
        let limit = limit.unwrap_or(usize::MAX);
        let mut materials = Vec::new();

        for (_, record) in records.iter() {
            // material 记录 + 异步提取写入的 learning memory 记录都要能被列出
            let material = match &record.material {
                Some(m) => Some(m.clone()),
                None => {
                    if is_learning_memory(&record.memory) {
                        Some(memory_to_material(&record.memory))
                    } else {
                        None
                    }
                }
            };

            if let Some(material) = material {
                // R10：只列出"技术类学习素材/经验数据集"
                if !crate::stores::vector::types::is_countable_material_type(&material.material_type) {
                    continue;
                }
                let matches_owner = match (&owner_type, owner_id) {
                    (Some(ot), Some(oid)) => material.owner_type == *ot && material.owner_id == oid,
                    (Some(ot), None) => material.owner_type == *ot,
                    (None, Some(oid)) => material.owner_id == oid,
                    (None, None) => true,
                };

                if matches_owner {
                    materials.push(material);
                    if materials.len() >= limit {
                        break;
                    }
                }
            }
        }

        Ok(materials)
    }

    pub async fn clear_materials(&self) -> Result<(), ShouxinError> {
        {
            let mut hnsw = self.hnsw.write().await;
            let max_elements = 10000;
            *hnsw = Hnsw::new(
                self.config.m,
                max_elements,
                16,
                self.config.ef_construction,
                DistCosine,
            );
        }
        {
            let mut records = self.records.write().await;
            records.clear();
        }
        {
            let mut id_to_uuid = self.id_to_uuid.write().await;
            let mut uuid_to_id = self.uuid_to_id.write().await;
            id_to_uuid.clear();
            uuid_to_id.clear();
        }
        self.id_counter.store(0, Ordering::SeqCst);
        self.db.clear().map_err(|e| ShouxinError::StorageError(format!("sled clear error: {}", e)))?;
        Ok(())
    }

    pub fn flush(&self) -> Result<(), ShouxinError> {
        self.db
            .flush()
            .map_err(|e| ShouxinError::StorageError(format!("sled flush error: {}", e)))?;
        Ok(())
    }

    pub async fn persist(&self) -> Result<usize, ShouxinError> {
        let count = self.records.read().await.len();
        tracing::info!("HNSW persist: flushing {} records to disk", count);
        self.db
            .flush_async()
            .await
            .map_err(|e| ShouxinError::StorageError(format!("sled flush_async error: {}", e)))?;
        tracing::info!("HNSW persist: {} records flushed successfully", count);
        Ok(count)
    }

    pub async fn count_loaded(&self) -> usize {
        self.records.read().await.len()
    }

    pub fn get_dimension(&self) -> usize {
        self.dimension
    }

    pub fn get_config(&self) -> &VectorIndexConfig {
        &self.config
    }
}

#[async_trait]
impl VectorStore for HnswVectorStore {
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

        let material = if let Some(material_type) = memory.metadata.get("material_type").and_then(|v| v.as_str()) {
            let owner_type = memory.metadata.get("owner_type")
                .and_then(|v| v.as_str())
                // 大小写不敏感：写入侧用 `format!("{:?}")` 产生 "Agent"/"User"，
                // 旧实现只匹配小写 "agent"，导致 Agent 作用域的向量被落成 User（F-VS7）。
                .and_then(crate::core::contact::OwnerType::parse)
                .unwrap_or(crate::core::contact::OwnerType::User);
            let owner_id = memory.metadata.get("owner_id")
                .and_then(|v| v.as_str())
                .unwrap_or("default_user")
                .to_string();

            let id = self.next_id() as u64;
            let mut mat = LearningMaterial::new_with_owner(
                id,
                memory.content.clone(),
                material_type.to_string(),
                owner_type,
                owner_id,
            );
            if let Some(v) = memory.metadata.get("summary").and_then(|v| v.as_str()) {
                mat = mat.with_summary(v.to_string());
            }
            if let Some(v) = memory.metadata.get("source_file_id").and_then(|v| v.as_str()) {
                mat = mat.with_source_conversation_id(v.to_string());
            }
            Some(mat)
        } else {
            None
        };

        let uuid = memory.id.clone();
        let point_id = self.next_id();

        let record = StoredRecord {
            memory: memory.clone(),
            vector: embedding.vector.clone(),
            material,
        };

        {
            let hnsw = self.hnsw.write().await;
            hnsw.insert_slice((&embedding.vector, point_id));
        }
        {
            let mut records = self.records.write().await;
            records.insert(point_id, record.clone());
        }
        {
            let mut id_to_uuid = self.id_to_uuid.write().await;
            let mut uuid_to_id = self.uuid_to_id.write().await;
            id_to_uuid.insert(point_id, uuid.clone());
            uuid_to_id.insert(uuid.clone(), point_id);
        }

        self.save_record(&uuid, &record)?;
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Memory>, ShouxinError> {
        let uuid_to_id = self.uuid_to_id.read().await;
        if let Some(&point_id) = uuid_to_id.get(id) {
            let records = self.records.read().await;
            Ok(records.get(&point_id).map(|r| r.memory.clone()))
        } else {
            Ok(None)
        }
    }

    async fn search(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        _filters: Option<HashMap<String, Value>>,
    ) -> Result<Vec<SearchResult>, ShouxinError> {
        let ef = self.config.ef_search;
        let knbn = limit * 2;

        let neighbours = {
            let hnsw = self.hnsw.read().await;
            hnsw.search(&query_embedding.vector, knbn, ef)
        };

        let records = self.records.read().await;
        let mut results = Vec::new();

        for n in &neighbours {
            // records 存在性兜底过滤：若该 id 在 records 中不存在，跳过该结果。
            let record = match records.get(&n.d_id) {
                Some(r) => r,
                None => {
                    tracing::debug!(
                        point_id = n.d_id,
                        "search skip: point_id not found in records (possibly deleted)"
                    );
                    continue;
                }
            };
            let similarity = Self::cosine_similarity(&query_embedding.vector, &record.vector);
            results.push(SearchResult::new(
                record.memory.clone(),
                similarity,
                crate::core::SearchSource::Vector,
            ));
            if results.len() >= limit {
                break;
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results)
    }

    async fn search_with_filter(
        &self,
        query_embedding: &Embedding,
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<SearchResult>, ShouxinError> {
        let ef = self.config.ef_search;
        let knbn = limit * 2;

        let neighbours = {
            let hnsw = self.hnsw.read().await;
            hnsw.search(&query_embedding.vector, knbn, ef)
        };

        let records = self.records.read().await;
        let mut results = Vec::new();

        for n in &neighbours {
            // records 存在性兜底过滤：若该 id 在 records 中不存在，跳过该结果。
            let record = match records.get(&n.d_id) {
                Some(r) => r,
                None => {
                    tracing::debug!(
                        point_id = n.d_id,
                        "search_with_filter skip: point_id not found in records (possibly deleted)"
                    );
                    continue;
                }
            };
            let matches_filter = filter.user_id.as_ref().is_none_or(|uid| {
                record.memory.user_id.as_deref() == Some(uid.as_str())
            });
            if matches_filter {
                let similarity = Self::cosine_similarity(&query_embedding.vector, &record.vector);
                results.push(SearchResult::new(
                    record.memory.clone(),
                    similarity,
                    crate::core::SearchSource::Vector,
                ));
            }
            if results.len() >= limit {
                break;
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results)
    }

    async fn delete(&self, memory_id: &str) -> Result<(), ShouxinError> {
        let uuid_to_id = self.uuid_to_id.read().await;
        if let Some(&point_id) = uuid_to_id.get(memory_id) {
            let mut records = self.records.write().await;
            records.remove(&point_id);
            drop(uuid_to_id);
            let mut id_to_uuid = self.id_to_uuid.write().await;
            let mut uuid_to_id_w = self.uuid_to_id.write().await;
            id_to_uuid.remove(&point_id);
            uuid_to_id_w.remove(memory_id);
        }
        self.db.remove(memory_id.as_bytes()).ok();
        Ok(())
    }

    async fn update(&self, memory: Memory) -> Result<(), ShouxinError> {
        let uuid = memory.id.clone();
        let uuid_to_id = self.uuid_to_id.read().await;
        if let Some(&point_id) = uuid_to_id.get(&uuid) {
            let mut records = self.records.write().await;
            if let Some(existing) = records.get_mut(&point_id) {
                existing.memory = memory;
            }
        }
        Ok(())
    }

    async fn count(&self, _filter: Option<&MemoryFilter>) -> Result<usize, ShouxinError> {
        Ok(self.records.read().await.len())
    }

    async fn list(&self, _filter: Option<&MemoryFilter>) -> Result<Vec<Memory>, ShouxinError> {
        let records = self.records.read().await;
        let memories: Vec<Memory> = records.values().map(|r| r.memory.clone()).collect();
        Ok(memories)
    }

    async fn clear(&self) -> Result<(), ShouxinError> {
        self.clear_materials().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contact::OwnerType;

    #[tokio::test]
    async fn test_hnsw_insert_and_search() {
        let store = HnswVectorStore::in_memory(768).unwrap();
        let start = std::time::Instant::now();

        let mut rng = rand::thread_rng();
        let mut ids = Vec::new();

        for i in 0..100 {
            let vector: Vec<f32> = (0..768).map(|_| rand::Rng::gen(&mut rng)).collect();
            let embedding = Embedding {
                vector,
                dimension: 768,
                model_name: "test".to_string(),
            };
            let content = format!("测试内容 {}", i);
            let id = store
                .insert_material_with_embedding(
                    &content,
                    None,
                    None,
                    "test",
                    &embedding,
                    OwnerType::User,
                    "test_user",
                )
                .await
                .unwrap();
            ids.push(id);
        }

        let insert_elapsed = start.elapsed();
        println!(
            "Inserted 100 records in {:?}, searching...",
            insert_elapsed
        );

        let search_start = std::time::Instant::now();
        let iterations = 100;
        let mut total_found = 0;

        let query_vector: Vec<f32> = (0..768).map(|_| rand::Rng::gen(&mut rng)).collect();
        let query_embedding = Embedding {
            vector: query_vector,
            dimension: 768,
                model_name: "test".to_string(),
        };

        for _ in 0..iterations {
            let results = store
                .semantic_search_with_embedding(&query_embedding, 5, None, None)
                .await
                .unwrap();
            total_found += results.len();
        }

        let search_elapsed = search_start.elapsed();
        let avg_ns = search_elapsed.as_nanos() / iterations as u128;
        let avg_ms = avg_ns as f64 / 1_000_000.0;

        println!(
            "HNSW search: {} iterations, avg {:.4} ms per search, found {} results",
            iterations, avg_ms, total_found
        );

        assert!(
            avg_ms < 10.0,
            "HNSW search should be < 10ms (debug), got {:.2}ms",
            avg_ms
        );
        assert_eq!(ids.len(), 100);
    }

    #[tokio::test]
    async fn test_hnsw_bulk_search_performance() {
        let store = HnswVectorStore::in_memory(768).unwrap();
        let mut rng = rand::thread_rng();

        for i in 0..1000 {
            let vector: Vec<f32> = (0..768).map(|_| rand::Rng::gen(&mut rng)).collect();
            let embedding = Embedding {
                vector,
                dimension: 768,
                model_name: "test".to_string(),
            };
            store
                .insert_material_with_embedding(
                    &format!("content_{}", i),
                    None,
                    None,
                    "test",
                    &embedding,
                    OwnerType::User,
                    "test",
                )
                .await
                .unwrap();
        }

        let query_vector: Vec<f32> = (0..768).map(|_| rand::Rng::gen(&mut rng)).collect();
        let query_embedding = Embedding {
            vector: query_vector,
            dimension: 768,
                model_name: "test".to_string(),
        };

        let iterations = 100;
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            store
                .semantic_search_with_embedding(&query_embedding, 10, None, None)
                .await
                .unwrap();
        }

        let elapsed = start.elapsed();
        let avg_ms = elapsed.as_secs_f64() * 1000.0 / iterations as f64;

        println!(
            "1000-vector HNSW: {} iterations, avg {:.3} ms",
            iterations, avg_ms
        );
        assert!(avg_ms < 10.0, "HNSW search 1000 vecs should be < 10ms (debug), got {:.2}ms", avg_ms);
    }
}
/// 回归测试：异步提取路径写入的 learning `Memory` 记录必须对素材接口可见
///
/// 复现的缺陷（F-VS6）：HNSW 的 `semantic_search_with_embedding` /
/// `get_material` / `count_materials` / `list_materials` 都只处理 `record.material`，
/// 而异步路径通过 `VectorStore::insert(Memory)` 写入的是 `record.memory`。
/// 修复前：检索返回 0 条、详情 404、计数为 0、列表为空。
#[cfg(test)]
mod material_visibility_tests {
    use super::*;
    use crate::core::{MemoryState, MemoryType};
    use serde_json::json;
    use chrono::Utc;

    fn learning_memory(id: &str, content: &str, owner: &str, owner_id: &str, dim: usize) -> Memory {
        let mut metadata = HashMap::new();
        metadata.insert("material_type".to_string(), json!("learning"));
        metadata.insert("owner_type".to_string(), json!(owner));
        metadata.insert("owner_id".to_string(), json!(owner_id));
        metadata.insert("source_conversation_id".to_string(), json!("conv-1"));

        Memory {
            id: id.to_string(),
            user_id: None,
            agent_id: None,
            content: content.to_string(),
            embedding: Some(Embedding {
                vector: vec![0.1f32; dim],
                dimension: dim,
                model_name: "test".to_string(),
            }),
            metadata,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            memory_type: MemoryType::LongTerm,
            state: MemoryState::Active,
            hash: None,
            score: 1.0,
        }
    }

    #[tokio::test]
    async fn test_memory_record_visible_to_material_apis() {
        let dim = 8;
        let store = HnswVectorStore::in_memory(dim).unwrap();
        store
            .insert(learning_memory("m1", "异步写入的学习素材", "Agent", "agent-1", dim))
            .await
            .unwrap();

        let query = Embedding {
            vector: vec![0.1f32; dim],
            dimension: dim,
            model_name: "test".to_string(),
        };

        // 1) 检索：修复前返回空
        let hits = store
            .semantic_search_with_embedding(&query, 5, Some(OwnerType::Agent), Some("agent-1"))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1, "memory 记录必须出现在素材检索结果中");
        assert_eq!(hits[0].material.content, "异步写入的学习素材");
        assert_eq!(hits[0].material.owner_id, "agent-1");

        // 2) owner 隔离仍然有效
        let other_owner = store
            .semantic_search_with_embedding(&query, 5, Some(OwnerType::User), Some("u1"))
            .await
            .unwrap();
        assert!(other_owner.is_empty(), "不同 owner 不应检索到该素材");

        // 3) 详情：修复前为 None（API 会 404）
        let detail = store.get_material("m1").await.unwrap();
        assert!(detail.is_some(), "memory 记录的素材详情不应为空");
        assert_eq!(detail.unwrap().owner_type, OwnerType::Agent);

        // 4) 计数与列表：修复前都会漏掉该记录
        assert_eq!(store.count_materials().await.unwrap(), 1);
        let listed = store
            .list_materials(None, Some(OwnerType::Agent), Some("agent-1"))
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].content, "异步写入的学习素材");
    }

    /// owner 解析大小写不敏感：`format!("{:?}")` 产生 "Agent"，
    /// 旧实现只认小写 "agent"，会把 Agent 作用域写成 User（F-VS7）
    #[tokio::test]
    async fn test_owner_parsing_is_case_insensitive() {
        let dim = 8;
        let store = HnswVectorStore::in_memory(dim).unwrap();
        store
            .insert(learning_memory("m2", "大小写测试", "agent", "agent-2", dim))
            .await
            .unwrap();
        store
            .insert(learning_memory("m3", "标准写法", "Agent", "agent-3", dim))
            .await
            .unwrap();

        let query = Embedding {
            vector: vec![0.1f32; dim],
            dimension: dim,
            model_name: "test".to_string(),
        };

        for (owner_id, content) in [("agent-2", "大小写测试"), ("agent-3", "标准写法")] {
            let hits = store
                .semantic_search_with_embedding(&query, 10, Some(OwnerType::Agent), Some(owner_id))
                .await
                .unwrap();
            assert_eq!(
                hits.len(),
                1,
                "owner_type={:?} 的记录必须能按 Agent 作用域检索到（{}）",
                OwnerType::Agent,
                content
            );
        }
    }
}

#[cfg(test)]
mod material_scope_tests {
    use super::*;
    use crate::core::{MemoryState, MemoryType};
    use chrono::Utc;
    use serde_json::json;

    fn memory_with_type(id: &str, material_type: &str, dim: usize) -> Memory {
        let mut metadata = HashMap::new();
        metadata.insert("material_type".to_string(), json!(material_type));
        metadata.insert("owner_type".to_string(), json!("User"));
        metadata.insert("owner_id".to_string(), json!("u1"));
        Memory {
            id: id.to_string(),
            user_id: None,
            agent_id: None,
            content: format!("content-{}", material_type),
            embedding: Some(Embedding {
                vector: vec![0.2f32; dim],
                dimension: dim,
                model_name: "test".to_string(),
            }),
            metadata,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            memory_type: MemoryType::LongTerm,
            state: MemoryState::Active,
            hash: None,
            score: 1.0,
        }
    }

    /// R10：素材统计/列表只包含"技术类学习素材、经验类数据集"，
    /// 不含事件、文件分块（文章/诗歌等以文件形式进入时为 file_chunk）。
    #[tokio::test]
    async fn test_material_scope_excludes_event_and_file_chunk() {
        let dim = 8;
        let store = HnswVectorStore::in_memory(dim).unwrap();
        store.insert(memory_with_type("l1", "learning", dim)).await.unwrap();
        store.insert(memory_with_type("e1", "event", dim)).await.unwrap();
        store.insert(memory_with_type("f1", "file_chunk", dim)).await.unwrap();

        assert_eq!(
            store.count_materials().await.unwrap(),
            1,
            "只有 learning 计入素材统计（event/file_chunk 不计）"
        );

        let listed = store.list_materials(None, None, None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].material_type, "learning");
    }

    /// 白名单包含未来可能细分的类型，且大小写不敏感
    #[tokio::test]
    async fn test_material_scope_allowlist() {
        use crate::stores::vector::types::is_countable_material_type;
        assert!(is_countable_material_type("learning"));
        assert!(is_countable_material_type("Learning"));
        assert!(is_countable_material_type("technical_learning"));
        assert!(is_countable_material_type("experience_dataset"));
        assert!(!is_countable_material_type("event"));
        assert!(!is_countable_material_type("file_chunk"));
        assert!(!is_countable_material_type("poetry"));
    }
}
