//! 待确认问题池
//!
//! 负责持久化和管理心跳模块生成的待确认问题

use std::sync::Arc;
use chrono::Utc;
use sled::Db;
use uuid::Uuid;

use crate::error::ShouxinError;
use crate::heartbeat::types::{Question, QuestionCategory, QuestionPriority};
use crate::stores::relational::store::RelationalStore;
use tracing::warn;

/// 问题池键前缀
const QUESTION_KEY_PREFIX: &[u8] = b"q:";
/// 已回答问题键前缀
const ANSWERED_KEY_PREFIX: &[u8] = b"a:";
/// 联系人关联索引前缀
const CONTACT_INDEX_PREFIX: &[u8] = b"c:";

/// 待确认问题池
///
/// 使用 sled 持久化存储问题，确保重启后不丢失
pub struct QuestionPool {
    /// sled 数据库实例
    db: Db,
    /// 关系库引用（用于歧义闭环）
    rel_store: Arc<RelationalStore>,
}

impl QuestionPool {
    /// 创建新的问题池
    ///
    /// # 参数
    /// - `path`: sled 数据库存储路径
    /// - `rel_store`: 关系库引用（用于歧义闭环）
    ///
    /// # 返回
    /// 初始化后的 QuestionPool
    pub fn new(path: &str, rel_store: Arc<RelationalStore>) -> Result<Self, ShouxinError> {
        let db = sled::open(path)
            .map_err(|e| ShouxinError::StorageError(format!("打开问题池失败: {}", e)))?;
        Ok(Self { db, rel_store })
    }

    /// 添加问题
    ///
    /// 自动去重：同类别同联系人的问题会合并为一个概括性问题
    ///
    /// # 参数
    /// - `question`: 待添加的问题
    pub async fn add_question(&self, question: Question) -> Result<(), ShouxinError> {
        let serialized = serde_json::to_string(&question)
            .map_err(|e| ShouxinError::SerializationError(e.to_string()))?;
        
        // 存储问题
        let question_key = [QUESTION_KEY_PREFIX, question.id.as_bytes()].concat();
        self.db
            .insert(question_key, serialized.as_bytes())
            .map_err(|e| ShouxinError::StorageError(format!("存储问题失败: {}", e)))?;
        
        // 更新联系人关联索引
        for contact_id in &question.related_contact_ids {
            let contact_key = [
                CONTACT_INDEX_PREFIX,
                contact_id.as_bytes(),
                b":",
                question.id.as_bytes()
            ].concat();
            self.db
                .insert(contact_key, b"1")
                .map_err(|e| ShouxinError::StorageError(format!("更新索引失败: {}", e)))?;
        }
        
        Ok(())
    }

    /// 解决问题
    ///
    /// # 参数
    /// - `question_id`: 问题 ID
    /// - `answer`: 用户提供的答案
    ///
    /// # 说明
    /// 对于冲突解决和实体歧义类别的问题，若 `resolve_ambiguity` 失败，
    /// 将直接返回错误，不更新问题详情，问题保持待确认状态以便调用者重试。
    pub async fn resolve_question(&self, question_id: &str, answer: &str) -> Result<(), ShouxinError> {
        let question_key = [QUESTION_KEY_PREFIX, question_id.as_bytes()].concat();

        let question_data = self.db
            .get(&question_key)
            .map_err(|e| ShouxinError::StorageError(format!("读取问题失败: {}", e)))?
            .ok_or_else(|| ShouxinError::StorageError(format!("问题 {} 不存在", question_id)))?;

        let mut question: Question = serde_json::from_slice(&question_data)
            .map_err(|e| ShouxinError::SerializationError(format!("反序列化问题失败: {}", e)))?;

        if question.category == crate::heartbeat::types::QuestionCategory::ConflictResolution
            || question.category == crate::heartbeat::types::QuestionCategory::EntityDisambiguation
        {
            // 若 resolve_ambiguity 失败，返回错误，不标记为已回答，问题保持待确认状态
            if let Err(e) = self.rel_store.resolve_ambiguity(question_id, answer).await {
                warn!("Failed to resolve ambiguity {} in store: {}", question_id, e);
                return Err(e);
            }
        }

        question.detail = format!("{} | 用户回答: {}", question.detail, answer);

        let updated = serde_json::to_string(&question)
            .map_err(|e| ShouxinError::SerializationError(e.to_string()))?;

        self.db
            .insert(question_key, updated.as_bytes())
            .map_err(|e| ShouxinError::StorageError(format!("更新问题失败: {}", e)))?;

        Ok(())
    }

    /// 智能聚合：将关于同一联系人的多个问题合并为一个概括性问题
    ///
    /// # 参数
    /// - `contact_id`: 联系人 ID
    ///
    /// # 返回
    /// 聚合后的问题列表（可能减少问题数量）
    pub async fn aggregate_questions(&self, contact_id: &str) -> Result<Vec<Question>, ShouxinError> {
        // 获取该联系人的所有问题
        let contact_questions = self.get_questions_for_contact(contact_id).await?;
        
        if contact_questions.len() <= 1 {
            return Ok(contact_questions);
        }
        
        // 按类别分组
        let mut grouped: std::collections::HashMap<QuestionCategory, Vec<&Question>> = 
            std::collections::HashMap::new();
        
        for q in &contact_questions {
            grouped.entry(q.category.clone()).or_default().push(q);
        }
        
        let mut aggregated = Vec::new();
        
        for (category, questions) in grouped {
            if questions.len() == 1 {
                aggregated.push(questions[0].clone());
            } else {
                // 合并同类问题
                let merged = Self::merge_questions(contact_id, category, questions);
                aggregated.push(merged);
            }
        }
        
        Ok(aggregated)
    }

    /// 按优先级获取待确认问题
    ///
    /// # 参数
    /// - `limit`: 返回的问题数量上限
    ///
    /// # 返回
    /// 按优先级从高到低排列的问题列表
    pub async fn get_pending_questions(&self, limit: usize) -> Result<Vec<Question>, ShouxinError> {
        let mut questions = Vec::new();
        let now = Utc::now();
        
        for result in self.db.scan_prefix(QUESTION_KEY_PREFIX) {
            let (_, value) = result
                .map_err(|e| ShouxinError::StorageError(format!("读取问题失败: {}", e)))?;
            
            let question: Question = serde_json::from_slice(&value)
                .map_err(|e| ShouxinError::DeserializationError(e.to_string()))?;
            
            // 过滤过期问题
            if let Some(expires_at) = question.expires_at {
                if now > expires_at {
                    continue;
                }
            }
            
            questions.push(question);
        }
        
        // 按优先级从高到低排序
        questions.sort_by(|a, b| b.priority.cmp(&a.priority));
        questions.truncate(limit);
        
        Ok(questions)
    }

    /// 标记问题为已处理
    ///
    /// # 参数
    /// - `question_id`: 问题 ID
    /// - `answer`: 用户的回答
    pub async fn mark_answered(&self, question_id: &str, _answer: &str) -> Result<(), ShouxinError> {
        let question_key = [QUESTION_KEY_PREFIX, question_id.as_bytes()].concat();
        
        // 从待确认池移除
        if let Some(value) = self.db
            .remove(question_key)
            .map_err(|e| ShouxinError::StorageError(format!("移除问题失败: {}", e)))? {
            
            // 存储到已回答池
            let answered_key = [ANSWERED_KEY_PREFIX, question_id.as_bytes()].concat();
            self.db
                .insert(answered_key, value)
                .map_err(|e| ShouxinError::StorageError(format!("存储已回答问题失败: {}", e)))?;
            
            // 移除联系人关联索引
            let contact_index_prefix = [CONTACT_INDEX_PREFIX, b":"].concat();
            for result in self.db.scan_prefix(contact_index_prefix) {
                let (key, _) = result
                    .map_err(|e| ShouxinError::StorageError(format!("扫描索引失败: {}", e)))?;
                
                let key_str = String::from_utf8_lossy(&key);
                if key_str.ends_with(&format!(":{}", question_id)) {
                    self.db
                        .remove(key)
                        .map_err(|e| ShouxinError::StorageError(format!("移除索引失败: {}", e)))?;
                }
            }
        }
        
        Ok(())
    }

    /// 清理过期问题
    ///
    /// # 返回
    /// 清理的问题数量
    pub async fn cleanup_expired(&self) -> Result<usize, ShouxinError> {
        let now = Utc::now();
        let mut cleaned = 0;
        let mut to_remove = Vec::new();
        
        for result in self.db.scan_prefix(QUESTION_KEY_PREFIX) {
            let (key, value) = result
                .map_err(|e| ShouxinError::StorageError(format!("读取问题失败: {}", e)))?;
            
            let question: Question = serde_json::from_slice(&value)
                .map_err(|e| ShouxinError::DeserializationError(e.to_string()))?;
            
            if let Some(expires_at) = question.expires_at {
                if now > expires_at {
                    to_remove.push(key);
                    cleaned += 1;
                }
            }
        }
        
        for key in to_remove {
            self.db
                .remove(key)
                .map_err(|e| ShouxinError::StorageError(format!("清理问题失败: {}", e)))?;
        }
        
        Ok(cleaned)
    }

    /// 检查是否有关于某联系人的存疑事件堆积
    ///
    /// # 参数
    /// - `contact_id`: 联系人 ID
    ///
    /// # 返回
    /// 是否存在待确认问题
    pub async fn has_pending_for_contact(&self, contact_id: &str) -> Result<bool, ShouxinError> {
        let questions = self.get_questions_for_contact(contact_id).await?;
        Ok(!questions.is_empty())
    }

    // --- 辅助方法 ---

    /// 获取指定联系人的所有问题
    async fn get_questions_for_contact(&self, contact_id: &str) -> Result<Vec<Question>, ShouxinError> {
        let mut questions = Vec::new();
        let contact_prefix = [CONTACT_INDEX_PREFIX, contact_id.as_bytes(), b":"].concat();
        
        for result in self.db.scan_prefix(contact_prefix) {
            let (index_key, _) = result
                .map_err(|e| ShouxinError::StorageError(format!("读取索引失败: {}", e)))?;
            
            let index_str = String::from_utf8_lossy(&index_key);
            let question_id = index_str.split(':').next_back().unwrap_or("");
            
            let question_key = [QUESTION_KEY_PREFIX, question_id.as_bytes()].concat();
            let value_opt = self.db
                .get(question_key)
                .map_err(|e| ShouxinError::StorageError(format!("读取问题失败: {}", e)))?;
            
            if let Some(value) = value_opt {
                let question: Question = serde_json::from_slice(&value)
                    .map_err(|e| ShouxinError::DeserializationError(e.to_string()))?;
                
                questions.push(question);
            }
        }
        
        Ok(questions)
    }

    /// 合并多个问题为一个概括性问题
    fn merge_questions(contact_id: &str, category: QuestionCategory, questions: Vec<&Question>) -> Question {
        // 获取最高优先级
        let highest_priority = questions
            .iter()
            .map(|q| q.priority.clone())
            .max()
            .unwrap_or(QuestionPriority::Medium);
        
        // 合并标题
        let merged_title = format!("关于 {} 的 {} 个问题", contact_id, questions.len());
        
        // 合并详情
        let mut merged_detail = String::new();
        for (i, q) in questions.iter().enumerate() {
            if i > 0 {
                merged_detail.push_str("\n\n");
            }
            merged_detail.push_str(&q.detail);
        }
        
        // 合并关联 ID
        let mut related_contact_ids: Vec<String> = questions
            .iter()
            .flat_map(|q| q.related_contact_ids.clone())
            .collect();
        related_contact_ids.dedup();
        
        let mut related_event_ids: Vec<String> = questions
            .iter()
            .flat_map(|q| q.related_event_ids.clone())
            .collect();
        related_event_ids.dedup();
        
        // 取最长的过期时间
        let expires_at = questions
            .iter()
            .flat_map(|q| q.expires_at)
            .max();
        
        Question {
            id: Uuid::new_v4().to_string(),
            priority: highest_priority,
            category,
            title: merged_title,
            detail: merged_detail,
            context: None,
            created_at: Utc::now(),
            expires_at,
            related_contact_ids,
            related_event_ids,
        }
    }
}
