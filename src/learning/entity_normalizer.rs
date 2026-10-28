//! 实体归一化引擎
//!
//! 统一同义实体（如大小写变体、缩写），减少检索冗余
//! 在向量库入库前进行实体识别与聚类

use chrono::{DateTime, Utc};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::ShouxinError;
use crate::traits::LlmClient;

/// 实体识别结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityMention {
    /// 原文提及
    pub mention: String,
    /// 标准名称（归一化后）
    pub canonical: String,
}

/// 实体别名记录
#[derive(Debug, Clone)]
pub struct EntityAlias {
    /// 标准实体ID
    pub canonical_id: String,
    /// 别名（如 "ragflow", "RAGFlow"）
    pub alias: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

/// 实体归一化器
///
/// 维护实体别名表，提供实体识别和归一化功能
pub struct EntityNormalizer {
    /// LLM 客户端（用于实体识别）
    llm_client: Arc<dyn LlmClient>,
    /// 内存缓存（LRU），key 是别名，value 是标准实体ID
    cache: Arc<RwLock<LruCache<String, String>>>,
    /// 缓存大小上限
    cache_capacity: usize,
}

impl EntityNormalizer {
    /// 创建新的实体归一化器
    ///
    /// # 参数
    /// - `llm_client`: LLM 客户端
    /// - `cache_capacity`: LRU 缓存容量
    ///
    /// # 返回
    /// 新的实体归一化器实例
    pub fn new(llm_client: Arc<dyn LlmClient>, cache_capacity: usize) -> Self {
        Self {
            llm_client,
            cache: Arc::new(RwLock::new(LruCache::new(NonZeroUsize::new(cache_capacity).unwrap_or(NonZeroUsize::MIN)))),
            cache_capacity,
        }
    }

    /// 识别文本中的实体并归一化
    ///
    /// # 参数
    /// - `content`: 输入文本
    ///
    /// # 返回
    /// 实体识别结果列表
    pub async fn normalize(&self, content: &str) -> Result<Vec<EntityMention>, ShouxinError> {
        let prompt = format!(
            r#"识别以下文本中的核心实体（人名、组织、技术术语、产品名等），并对每个实体给出标准名称（统一大小写和缩写）。

**重要语言约束：mention 和 canonical 字段必须使用中文（专有名词除外），禁止使用英语、西班牙语或其他语言。**

文本：{}

输出 JSON 格式：[{{"mention": "原文提及", "canonical": "标准名"}}]"#,
            content
        );

        let message = crate::core::Message::user(prompt)
            .map_err(|e| ShouxinError::InvalidArgument(format!("创建消息失败: {}", e)))?;

        let response = self.llm_client.chat(vec![message]).await?;

        let entities: Vec<EntityMention> = serde_json::from_str(&response)
            .map_err(|e| ShouxinError::InvalidArgument(format!("解析实体识别结果失败: {}", e)))?;

        Ok(entities)
    }

    /// 根据别名查找标准实体ID
    ///
    /// # 参数
    /// - `alias`: 别名
    ///
    /// # 返回
    /// 标准实体ID（如果存在）
    pub async fn lookup(&self, alias: &str) -> Option<String> {
        let mut cache = self.cache.write().await;
        cache.get(&alias.to_string()).cloned()
    }

    /// 插入新的别名映射
    ///
    /// # 参数
    /// - `canonical_id`: 标准实体ID
    /// - `alias`: 别名
    pub async fn insert(&self, canonical_id: &str, alias: &str) {
        let mut cache = self.cache.write().await;
        cache.put(alias.to_string(), canonical_id.to_string());
    }

    /// 批量插入别名映射
    ///
    /// # 参数
    /// - `aliases`: 别名记录列表
    pub async fn insert_batch(&self, aliases: &[EntityAlias]) {
        let mut cache = self.cache.write().await;
        for alias_record in aliases {
            cache.put(alias_record.alias.clone(), alias_record.canonical_id.clone());
        }
    }

    /// 清除缓存
    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }

    /// 获取缓存统计信息
    pub async fn cache_stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        CacheStats {
            size: cache.len(),
            capacity: self.cache_capacity,
        }
    }

    /// 生成新的标准实体ID
    ///
    /// # 参数
    /// - `canonical_name`: 标准实体名称
    ///
    /// # 返回
    /// 新的实体ID
    pub fn generate_canonical_id(canonical_name: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        canonical_name.to_lowercase().hash(&mut hasher);
        format!("entity_{:x}", hasher.finish())
    }
}

/// 缓存统计信息
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// 当前缓存大小
    pub size: usize,
    /// 缓存容量上限
    pub capacity: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_canonical_id() {
        let id1 = EntityNormalizer::generate_canonical_id("RAGFlow");
        let id2 = EntityNormalizer::generate_canonical_id("ragflow");
        assert_eq!(id1, id2, "相同名称的不同大小写应生成相同ID");
    }
}
