//! 学习模块
//!
//! 提供主动学习与知识管理功能
//!
//! # 模块组成
//! - entity_normalizer: 实体归一化引擎
//! - slice_relations: 切片关联关系构建
//! - summarizer: 关键词与摘要生成
//! - knowledge_graph: 知识图谱构建
//! - learning_logger: 学习日志系统

pub mod entity_normalizer;
pub mod knowledge_graph;
pub mod learning_logger;
pub mod slice_relations;
pub mod summarizer;

pub use entity_normalizer::{EntityAlias, EntityMention, EntityNormalizer};
pub use knowledge_graph::{KnowledgeTriple, KnowledgeGraphExtractor};
pub use learning_logger::{ErrorEntry, FeatureRequestEntry, LearningEntry, LearningStatus, LogType, LearningLogger, Priority};
pub use slice_relations::{SliceRelation, SliceRelationsBuilder};
pub use summarizer::{Summarizer, SummaryResult};
