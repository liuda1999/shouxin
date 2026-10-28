//! 向量库数据类型
//!
//! 定义向量库使用的数据结构和类型

use chrono::DateTime;
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// 学习素材元数据（存入 sled）
///
/// 存储高质量学习素材的元数据，用于向量库的搜索和管理
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningMaterial {
    /// 唯一 ID（自增或 UUID）
    pub id: u64,
    /// 原始文本内容
    pub content: String,
    /// 内容摘要（LLM生成，不超过50字）
    pub summary: Option<String>,
    /// 来源对话 ID（指向字典库，便于回溯）
    pub source_conversation_id: Option<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 素材类型（如 "knowledge", "fact", "preference"）
    pub material_type: String,
    /// 归一化后的实体ID
    pub canonical_entity_id: Option<String>,
    /// 提取的关键词列表
    pub keywords: Vec<String>,
    /// 前序切片ID（上下文关联）
    pub context_before_id: Option<String>,
    /// 后序切片ID（上下文关联）
    pub context_after_id: Option<String>,
    /// 抽取的知识三元组
    pub triples: Vec<crate::learning::KnowledgeTriple>,
    /// 所有者类型
    pub owner_type: crate::core::contact::OwnerType,
    /// 所有者 ID
    pub owner_id: String,
}

impl LearningMaterial {
    /// 创建新的学习素材
    ///
    /// # 参数
    /// - `id`: 唯一标识符
    /// - `content`: 原始文本内容
    /// - `material_type`: 素材类型
    ///
    /// # 返回
    /// 新的学习素材实例
    pub fn new(id: u64, content: String, material_type: String) -> Self {
        Self {
            id,
            content,
            summary: None,
            source_conversation_id: None,
            created_at: Utc::now(),
            material_type,
            canonical_entity_id: None,
            keywords: Vec::new(),
            context_before_id: None,
            context_after_id: None,
            triples: Vec::new(),
            owner_type: crate::core::contact::OwnerType::User,
            owner_id: "default_user".to_string(),
        }
    }

    /// 创建新的学习素材（带所有者信息）
    pub fn new_with_owner(
        id: u64,
        content: String,
        material_type: String,
        owner_type: crate::core::contact::OwnerType,
        owner_id: String,
    ) -> Self {
        Self {
            id,
            content,
            summary: None,
            source_conversation_id: None,
            created_at: Utc::now(),
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

    /// 设置内容摘要
    pub fn with_summary(mut self, summary: String) -> Self {
        self.summary = Some(summary);
        self
    }

    /// 设置来源对话 ID
    pub fn with_source_conversation_id(mut self, source_conversation_id: String) -> Self {
        self.source_conversation_id = Some(source_conversation_id);
        self
    }

    /// 设置归一化实体ID
    pub fn with_canonical_entity_id(mut self, canonical_entity_id: String) -> Self {
        self.canonical_entity_id = Some(canonical_entity_id);
        self
    }

    /// 设置关键词
    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = keywords;
        self
    }

    /// 设置上下文关联
    pub fn with_context(
        mut self,
        context_before_id: Option<String>,
        context_after_id: Option<String>,
    ) -> Self {
        self.context_before_id = context_before_id;
        self.context_after_id = context_after_id;
        self
    }

    /// 设置知识三元组
    pub fn with_triples(mut self, triples: Vec<crate::learning::KnowledgeTriple>) -> Self {
        self.triples = triples;
        self
    }
}

/// 向量搜索结果
///
/// 表示向量搜索的结果，包含学习素材和相似度分数
#[derive(Debug, Clone, Serialize)]
pub struct VectorSearchResult {
    /// 学习素材
    pub material: LearningMaterial,
    /// 相似度分数（0-1，余弦相似度，分数越高越相似）
    pub score: f32,
}

impl VectorSearchResult {
    /// 创建新的向量搜索结果
    ///
    /// # 参数
    /// - `material`: 学习素材
    /// - `score`: 相似度分数
    ///
    /// # 返回
    /// 新的向量搜索结果实例
    pub fn new(material: LearningMaterial, score: f32) -> Self {
        Self { material, score }
    }
}

/// 向量库索引配置
///
/// 配置向量库的索引参数
#[derive(Debug, Clone)]
pub struct VectorIndexConfig {
    /// 存储类型（"embedded" 或 "hnsw"）
    pub store_type: String,
    /// 嵌入维度
    pub dimension: usize,
    /// 距离度量类型
    pub distance_metric: crate::traits::vector_store::DistanceMetric,
    /// M 参数（HNSW 算法参数）
    pub m: usize,
    /// efConstruction 参数（HNSW 算法参数）
    pub ef_construction: usize,
    /// efSearch 参数（HNSW 算法参数）
    pub ef_search: usize,
}

impl Default for VectorIndexConfig {
    fn default() -> Self {
        Self {
            store_type: "embedded".to_string(),
            dimension: 768, // 默认 768 维（BAAI/bge-base-en-v1.5）
            distance_metric: crate::traits::vector_store::DistanceMetric::Cosine,
            m: 16,                // 默认 M 值
            ef_construction: 100, // 默认 efConstruction 值
            ef_search: 10,        // 默认 efSearch 值
        }
    }
}

impl VectorIndexConfig {
    /// 设置嵌入维度
    pub fn with_dimension(mut self, dimension: usize) -> Self {
        self.dimension = dimension;
        self
    }

    /// 设置距离度量类型
    pub fn with_distance_metric(
        mut self,
        distance_metric: crate::traits::vector_store::DistanceMetric,
    ) -> Self {
        self.distance_metric = distance_metric;
        self
    }

    /// 设置 M 参数
    pub fn with_m(mut self, m: usize) -> Self {
        self.m = m;
        self
    }

    /// 设置 efConstruction 参数
    pub fn with_ef_construction(mut self, ef_construction: usize) -> Self {
        self.ef_construction = ef_construction;
        self
    }

    /// 设置 efSearch 参数
    pub fn with_ef_search(mut self, ef_search: usize) -> Self {
        self.ef_search = ef_search;
        self
    }
}

/// ID 映射条目
///
/// 用于维护业务 UUID 与 usearch 整数 ID 的映射关系
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdMapping {
    /// 业务 UUID
    pub business_id: String,
    /// usearch 整数 ID
    pub usearch_id: u64,
}

impl IdMapping {
    /// 创建新的 ID 映射
    ///
    /// # 参数
    /// - `business_id`: 业务 UUID
    /// - `usearch_id`: usearch 整数 ID
    ///
    /// # 返回
    /// 新的 ID 映射实例
    pub fn new(business_id: String, usearch_id: u64) -> Self {
        Self {
            business_id,
            usearch_id,
        }
    }
}

// ---------------------------------------------------------------------------
// 学习素材类型口径（R10）
// ---------------------------------------------------------------------------

/// 计入"学习素材"统计与列表的类型白名单
///
/// 口径（用户定义）：**技术类学习素材、经验类数据集**。
/// 不含：常规泛用文本、文章、诗歌、事件、文件分块。
///
/// 说明：当前提取器产出的学习素材统一标记为 `learning`，
/// 因此它代表"技术/经验"这一档；若后续在提取阶段细分，直接把新类型加进来即可。
pub const COUNTED_MATERIAL_TYPES: &[&str] = &["learning", "technical_learning", "experience_dataset"];

/// 该 `material_type` 是否计入学习素材（大小写不敏感）
pub fn is_countable_material_type(material_type: &str) -> bool {
    COUNTED_MATERIAL_TYPES
        .iter()
        .any(|t| t.eq_ignore_ascii_case(material_type))
}
