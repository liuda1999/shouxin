use std::sync::Arc;
use crate::core::{Embedding, Memory, MemoryFilter, SearchResult, contact::OwnerType};
use crate::error::ShouxinError;
use crate::stores::vector::store::EmbeddedVectorStore;
use crate::stores::vector::hnsw_store::HnswVectorStore;
use crate::stores::vector::types::{LearningMaterial, VectorSearchResult};
use crate::traits::vector_store::VectorStore;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone)]
pub enum AnyVectorStore {
    Embedded(Arc<EmbeddedVectorStore>),
    Hnsw(Arc<HnswVectorStore>),
}

impl AnyVectorStore {
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
        match self {
            Self::Embedded(s) => s.insert_material_with_embedding(content, summary, source_id, material_type, embedding, owner_type, owner_id).await,
            Self::Hnsw(s) => s.insert_material_with_embedding(content, summary, source_id, material_type, embedding, owner_type, owner_id).await,
        }
    }

    pub async fn semantic_search_with_embedding(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        owner_type: Option<OwnerType>,
        owner_id: Option<&str>,
    ) -> Result<Vec<VectorSearchResult>, ShouxinError> {
        match self {
            Self::Embedded(s) => s.semantic_search_with_embedding(query_embedding, limit, owner_type, owner_id).await,
            Self::Hnsw(s) => s.semantic_search_with_embedding(query_embedding, limit, owner_type, owner_id).await,
        }
    }

    pub async fn get_material(&self, id: &str) -> Result<Option<LearningMaterial>, ShouxinError> {
        match self {
            Self::Embedded(s) => s.get_material(id).await,
            Self::Hnsw(s) => s.get_material(id).await,
        }
    }

    pub async fn delete_material(&self, id: &str) -> Result<(), ShouxinError> {
        match self {
            Self::Embedded(s) => s.delete_material(id).await,
            Self::Hnsw(s) => s.delete_material(id).await,
        }
    }

    pub async fn count_materials(&self) -> Result<usize, ShouxinError> {
        match self {
            Self::Embedded(s) => s.count_materials().await,
            Self::Hnsw(s) => s.count_materials().await,
        }
    }

    pub async fn list_materials(
        &self,
        limit: Option<usize>,
        owner_type: Option<OwnerType>,
        owner_id: Option<&str>,
    ) -> Result<Vec<LearningMaterial>, ShouxinError> {
        match self {
            Self::Embedded(s) => s.list_materials(limit, owner_type, owner_id).await,
            Self::Hnsw(s) => s.list_materials(limit, owner_type, owner_id).await,
        }
    }

    pub async fn clear_materials(&self) -> Result<(), ShouxinError> {
        match self {
            Self::Embedded(s) => s.clear_materials().await,
            Self::Hnsw(s) => s.clear_materials().await,
        }
    }

    pub fn flush(&self) -> Result<(), ShouxinError> {
        match self {
            Self::Embedded(s) => s.flush(),
            Self::Hnsw(s) => s.flush(),
        }
    }

    pub async fn persist(&self) -> Result<usize, ShouxinError> {
        match self {
            Self::Embedded(_s) => Ok(0),
            Self::Hnsw(s) => s.persist().await,
        }
    }

    pub async fn load_existing(&self) -> Result<usize, ShouxinError> {
        match self {
            Self::Embedded(_s) => Ok(0),
            Self::Hnsw(s) => s.load_existing().await,
        }
    }
}

#[async_trait]
impl VectorStore for AnyVectorStore {
    async fn insert(&self, memory: Memory) -> Result<(), ShouxinError> {
        match self {
            Self::Embedded(s) => s.insert(memory).await,
            Self::Hnsw(s) => s.insert(memory).await,
        }
    }

    async fn get(&self, id: &str) -> Result<Option<Memory>, ShouxinError> {
        match self {
            Self::Embedded(s) => s.get(id).await,
            Self::Hnsw(s) => s.get(id).await,
        }
    }

    async fn search(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        filters: Option<HashMap<String, Value>>,
    ) -> Result<Vec<SearchResult>, ShouxinError> {
        match self {
            Self::Embedded(s) => s.search(query_embedding, limit, filters).await,
            Self::Hnsw(s) => s.search(query_embedding, limit, filters).await,
        }
    }

    async fn search_with_filter(
        &self,
        query_embedding: &Embedding,
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<SearchResult>, ShouxinError> {
        match self {
            Self::Embedded(s) => s.search_with_filter(query_embedding, filter, limit).await,
            Self::Hnsw(s) => s.search_with_filter(query_embedding, filter, limit).await,
        }
    }

    async fn delete(&self, memory_id: &str) -> Result<(), ShouxinError> {
        match self {
            Self::Embedded(s) => s.delete(memory_id).await,
            Self::Hnsw(s) => s.delete(memory_id).await,
        }
    }

    async fn update(&self, memory: Memory) -> Result<(), ShouxinError> {
        match self {
            Self::Embedded(s) => s.update(memory).await,
            Self::Hnsw(s) => s.update(memory).await,
        }
    }

    async fn count(&self, filter: Option<&MemoryFilter>) -> Result<usize, ShouxinError> {
        match self {
            Self::Embedded(s) => s.count(filter).await,
            Self::Hnsw(s) => s.count(filter).await,
        }
    }

    async fn list(&self, filter: Option<&MemoryFilter>) -> Result<Vec<Memory>, ShouxinError> {
        match self {
            Self::Embedded(s) => s.list(filter).await,
            Self::Hnsw(s) => s.list(filter).await,
        }
    }

    async fn clear(&self) -> Result<(), ShouxinError> {
        match self {
            Self::Embedded(s) => s.clear().await,
            Self::Hnsw(s) => s.clear().await,
        }
    }
}