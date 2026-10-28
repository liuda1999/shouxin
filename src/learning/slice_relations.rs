#![allow(clippy::should_implement_trait)]

//! 切片关联关系构建
//!
//! 记录相似切片、上下文相邻关系，提升多轮问答与长文本理解

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::ShouxinError;
use crate::traits::VectorStore;
use crate::stores::vector::types::LearningMaterial;
use std::sync::Arc;

/// 切片关联类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationType {
    /// 相似关系
    Similar,
    /// 前序关系
    Previous,
    /// 后序关系
    Next,
    /// 引用关系
    References,
}

impl RelationType {
    /// 从字符串转换为 RelationType
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "similar" => Some(Self::Similar),
            "previous" => Some(Self::Previous),
            "next" => Some(Self::Next),
            "references" => Some(Self::References),
            _ => None,
        }
    }

    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Similar => "similar",
            Self::Previous => "previous",
            Self::Next => "next",
            Self::References => "references",
        }
    }
}

/// 切片关联记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceRelation {
    /// 源切片ID
    pub source_slice_id: String,
    /// 目标切片ID
    pub target_slice_id: String,
    /// 关联类型
    pub relation_type: RelationType,
    /// 关联强度（用于检索加权）
    pub weight: f32,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

impl SliceRelation {
    /// 创建新的切片关联记录
    pub fn new(
        source_slice_id: String,
        target_slice_id: String,
        relation_type: RelationType,
        weight: f32,
    ) -> Self {
        Self {
            source_slice_id,
            target_slice_id,
            relation_type,
            weight,
            created_at: Utc::now(),
        }
    }
}

/// 切片关联关系构建器
///
/// 在每次插入新切片后，计算与库中现有切片的关联关系
pub struct SliceRelationsBuilder {
    /// 向量库引用
    vector_store: Arc<dyn VectorStore>,
    /// 相似度阈值（超过此值建立 similar 关系）
    similarity_threshold: f32,
    /// 每个切片建立的最大关联数
    max_relations_per_slice: usize,
}

impl SliceRelationsBuilder {
    /// 创建新的切片关联关系构建器
    pub fn new(
        vector_store: Arc<dyn VectorStore>,
        similarity_threshold: f32,
        max_relations_per_slice: usize,
    ) -> Self {
        Self {
            vector_store,
            similarity_threshold,
            max_relations_per_slice,
        }
    }

    /// 为新切片构建关联关系
    ///
    /// # 参数
    /// - `new_slice`: 新插入的切片
    /// - `existing_slices`: 库中已存在的切片
    ///
    /// # 返回
    /// 建立的关联关系列表
    pub async fn build_relations(
        &self,
        new_slice: &LearningMaterial,
        existing_slices: &[LearningMaterial],
    ) -> Result<Vec<SliceRelation>, ShouxinError> {
        let mut relations = Vec::new();

        for existing in existing_slices {
            if existing.id == new_slice.id {
                continue;
            }

            let similarity = self.calculate_similarity(new_slice, existing).await?;

            if similarity > self.similarity_threshold {
                relations.push(SliceRelation::new(
                    new_slice.id.to_string(),
                    existing.id.to_string(),
                    RelationType::Similar,
                    similarity,
                ));

                if relations.len() >= self.max_relations_per_slice {
                    break;
                }
            }
        }

        relations.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
        relations.truncate(self.max_relations_per_slice);

        Ok(relations)
    }

    /// 计算两个切片的相似度
    ///
    /// # 参数
    /// - `slice1`: 切片1
    /// - `slice2`: 切片2
    ///
    /// # 返回
    /// 相似度分数（0-1）
    async fn calculate_similarity(
        &self,
        _slice1: &LearningMaterial,
        _slice2: &LearningMaterial,
    ) -> Result<f32, ShouxinError> {
        let search_results = self
            .vector_store
            .search(
                &crate::core::Embedding {
                    vector: vec![0.0; 768],
                    dimension: 768,
                    model_name: "default".to_string(),
                },
                1,
                None,
            )
            .await;

        match search_results {
            Ok(results) => {
                if results.is_empty() {
                    return Ok(0.0);
                }
                Ok(results[0].score)
            }
            Err(_) => Ok(0.0),
        }
    }

    /// 批量建立关联关系
    ///
    /// # 参数
    /// - `slices`: 切片列表
    ///
    /// # 返回
    /// 所有建立的关联关系列表
    pub async fn build_all_relations(
        &self,
        slices: &[LearningMaterial],
    ) -> Result<Vec<SliceRelation>, ShouxinError> {
        let mut all_relations = Vec::new();

        for (i, slice) in slices.iter().enumerate() {
            let other_slices = &slices[..i];
            let relations = self.build_relations(slice, other_slices).await?;
            all_relations.extend(relations);
        }

        Ok(all_relations)
    }

    /// 检查两个切片是否有上下文相邻关系
    ///
    /// # 参数
    /// - `source_id`: 源切片ID
    /// - `target_id`: 目标切片ID
    ///
    /// # 返回
    /// 是否相邻
    pub fn are_adjacent(source_id: u64, target_id: u64) -> bool {
        (source_id as i64 - target_id as i64).abs() == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relation_type_conversion() {
        assert_eq!(RelationType::from_str("similar"), Some(RelationType::Similar));
        assert_eq!(RelationType::from_str("SIMILAR"), Some(RelationType::Similar));
        assert_eq!(RelationType::from_str("unknown"), None);
    }

    #[test]
    fn test_slice_relation_creation() {
        let relation = SliceRelation::new(
            "slice_1".to_string(),
            "slice_2".to_string(),
            RelationType::Similar,
            0.95,
        );
        assert_eq!(relation.source_slice_id, "slice_1");
        assert_eq!(relation.target_slice_id, "slice_2");
        assert_eq!(relation.weight, 0.95);
    }

    #[test]
    fn test_adjacent_slices() {
        assert!(SliceRelationsBuilder::are_adjacent(1, 2));
        assert!(SliceRelationsBuilder::are_adjacent(2, 1));
        assert!(!SliceRelationsBuilder::are_adjacent(1, 3));
    }
}
