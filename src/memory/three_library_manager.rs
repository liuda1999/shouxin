//! 三库记忆管理器
//!
//! 基于三库架构的记忆管理核心实现
//!
//! # 架构说明
//! - 字典库：永久存储所有记忆数据
//! - 关系库：存储轻量索引，支持快速检索
//! - 向量库：存储语义嵌入，支持语义搜索
//! - 数据分流器：协调三库数据流转

use chrono::{DateTime, Utc};
use std::sync::Arc;

use crate::core::Embedding;
use crate::core::contact::Scope;
use crate::config::settings::{EventSettings, ExtractionSettings};
use crate::error::ShouxinError;
use crate::memory::any_vector_store::AnyVectorStore;
use crate::memory_extractor::{DataDispatcher, ExtractedElements};
use crate::service::background_queue::{BackgroundTaskQueue, ExtractionTask};
use crate::stores::relational::{RelationalStore, TodoListStore};
use crate::stores::vector::types::{LearningMaterial, VectorSearchResult};
use crate::traits::store_traits::{ContactDetail, Conversation, EventDetail, EventFilter};
use crate::traits::{Embedder, LlmClient, VectorStore};
use crate::utils::parse_event_time;

/// 添加记忆的结果
#[derive(Debug, Clone)]
pub struct AddResult {
    /// 新增的联系人
    pub contacts: Vec<ContactDetail>,
    /// 新增的事件
    pub events: Vec<EventDetail>,
    /// 新增的学习素材
    pub learnings: Vec<LearningMaterial>,
    /// 对话 ID
    pub conversation_id: Option<String>,
}

#[allow(clippy::derivable_impls)]
impl Default for AddResult {
    fn default() -> Self {
        Self {
            contacts: Vec::new(),
            events: Vec::new(),
            learnings: Vec::new(),
            conversation_id: None,
        }
    }
}

/// 三库记忆管理器配置
#[derive(Debug, Clone, Default)]
pub struct ThreeLibraryMemoryConfig {
    /// 是否启用向量库
    pub enable_vector_store: bool,
    /// 是否启用自动清理
    pub enable_auto_cleanup: bool,
    /// 清理间隔（秒）
    pub cleanup_interval: u64,
    /// 用户ID（默认）
    pub user_id: String,
    /// 主智能体ID
    pub agent_id: String,
    /// 事件热表容量配置
    pub event_settings: EventSettings,
    /// 提取流水线配置
    pub extraction_settings: ExtractionSettings,
}

impl ThreeLibraryMemoryConfig {
    /// 创建新的配置
    pub fn new() -> Self {
        Self {
            enable_vector_store: false,
            enable_auto_cleanup: false,
            cleanup_interval: 0,
            user_id: "default_user".to_string(),
            agent_id: "primary_agent_default".to_string(),
            event_settings: EventSettings::default(),
            extraction_settings: ExtractionSettings::default(),
        }
    }

    /// 设置是否启用向量库
    pub fn with_vector_store(mut self, enable: bool) -> Self {
        self.enable_vector_store = enable;
        self
    }

    /// 设置是否启用自动清理
    pub fn with_auto_cleanup(mut self, enable: bool) -> Self {
        self.enable_auto_cleanup = enable;
        self
    }

    /// 设置清理间隔
    pub fn with_cleanup_interval(mut self, interval: u64) -> Self {
        self.cleanup_interval = interval;
        self
    }
    
    /// 设置用户ID
    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = user_id;
        self
    }
    
    /// 设置主智能体ID
    pub fn with_agent_id(mut self, agent_id: String) -> Self {
        self.agent_id = agent_id;
        self
    }

    /// 设置事件热表容量配置
    pub fn with_event_settings(mut self, event_settings: EventSettings) -> Self {
        self.event_settings = event_settings;
        self
    }

    /// 设置提取流水线配置
    pub fn with_extraction_settings(mut self, extraction_settings: ExtractionSettings) -> Self {
        self.extraction_settings = extraction_settings;
        self
    }
}

/// 三库记忆管理器
///
/// 协调三库操作，提供统一的记忆管理接口
#[allow(dead_code)]
pub struct ThreeLibraryMemoryManager {
    /// 字典库
    dictionary_store: Arc<dyn crate::traits::store_traits::DictionaryStoreTrait>,
    /// 关系库
    relational_store: Arc<RelationalStore>,
    /// 向量库
    vector_store: Arc<dyn VectorStore>,
    /// 嵌入式向量库（用于语义搜索）
    embedded_vector_store: Option<Arc<AnyVectorStore>>,
    /// LLM 客户端（用于提取四要素）
    llm_client: Arc<dyn LlmClient>,
    /// 嵌入生成器（用于语义搜索）
    embedder: Arc<dyn Embedder>,
    /// 记忆提取器
    extractor: Arc<dyn crate::memory_extractor::extractor::MemoryExtractor>,
    /// 数据分流器（协调三库数据流转）
    /// 数据分流器（**历史遗留**，当前无调用方）
    ///
    /// `DataDispatcher::dispatch()` 在代码里从未被调用：同步路径在 `add()` 内联实现，
    /// 异步路径在 `BackgroundTaskQueue`，写入语义现已收敛到
    /// [`crate::memory::MemoryWriter`]。该字段仅为兼容外部构造调用而保留，
    /// 待阶段 4 把事件/素材写入也迁入 `MemoryWriter` 后删除。
    #[allow(dead_code)]
    data_dispatcher: Arc<DataDispatcher>,
    /// 配置
    config: ThreeLibraryMemoryConfig,
    /// 后台任务队列 (v0.5.1)
    background_queue: Option<Arc<BackgroundTaskQueue>>,
}

impl ThreeLibraryMemoryManager {
    /// 根据Scope获取对应的所有者ID
    fn get_owner_id(&self, scope: &Scope) -> String {
        match scope {
            Scope::User => self.config.user_id.clone(),
            Scope::Agent => self.config.agent_id.clone(),
        }
    }
    
    /// 根据Scope获取对应的所有者类型
    fn get_owner_type(&self, scope: &Scope) -> crate::core::contact::OwnerType {
        scope.to_owner_type()
    }

    /// 设置配置
    pub fn set_config(&mut self, config: ThreeLibraryMemoryConfig) {
        self.config = config;
    }

    /// 创建新的三库记忆管理器
    pub fn new(
        dictionary_store: Arc<dyn crate::traits::store_traits::DictionaryStoreTrait>,
        relational_store: Arc<RelationalStore>,
        vector_store: Arc<dyn VectorStore>,
        llm_client: Arc<dyn LlmClient>,
        embedder: Arc<dyn Embedder>,
        extractor: Arc<dyn crate::memory_extractor::extractor::MemoryExtractor>,
        data_dispatcher: Arc<DataDispatcher>,
    ) -> Self {
        Self {
            dictionary_store,
            relational_store,
            vector_store,
            embedded_vector_store: None,
            llm_client,
            embedder,
            extractor,
            data_dispatcher,
            config: ThreeLibraryMemoryConfig::default(),
            background_queue: None,
        }
    }

    /// 创建带有嵌入式向量库的三库记忆管理器
    #[allow(clippy::too_many_arguments)]
    pub fn with_embedded_vector_store(
        dictionary_store: Arc<dyn crate::traits::store_traits::DictionaryStoreTrait>,
        relational_store: Arc<RelationalStore>,
        vector_store: Arc<dyn VectorStore>,
        embedded_vector_store: Arc<AnyVectorStore>,
        llm_client: Arc<dyn LlmClient>,
        embedder: Arc<dyn Embedder>,
        extractor: Arc<dyn crate::memory_extractor::extractor::MemoryExtractor>,
        data_dispatcher: Arc<DataDispatcher>,
    ) -> Self {
        Self {
            dictionary_store,
            relational_store,
            vector_store,
            embedded_vector_store: Some(embedded_vector_store),
            llm_client,
            embedder,
            extractor,
            data_dispatcher,
            config: ThreeLibraryMemoryConfig::default(),
            background_queue: None,
        }
    }

    /// 设置嵌入式向量库
    pub fn set_embedded_vector_store(&mut self, store: Arc<AnyVectorStore>) {
        self.embedded_vector_store = Some(store);
    }

    /// 设置后台任务队列 (v0.5.1)
    pub fn set_background_queue(&mut self, queue: Arc<BackgroundTaskQueue>) {
        self.background_queue = Some(queue);
    }

    /// 重放指定对话的提取任务
    ///
    /// 用途：写入失败/进程中断后，对话会停在 `failed` 且派生数据已被补偿清理。
    /// 这里把字典库里的原文重新入队（状态回到 `pending`），无需重新提交对话。
    ///
    /// 约束：不删除/不修改字典原文；重复重放由写入侧的幂等与去重规则兜底。
    pub async fn replay_conversation(&self, conversation_id: &str) -> Result<(), ShouxinError> {
        let queue = self.background_queue.as_ref().ok_or_else(|| {
            ShouxinError::GeneralError("background queue not configured".to_string())
        })?;

        let conv = self
            .dictionary_store
            .get_conversation_by_id(conversation_id)
            .await?
            .ok_or_else(|| {
                ShouxinError::GeneralError(format!("conversation not found: {}", conversation_id))
            })?;

        // owner 由对话记录还原：有 agent_id 视为 Agent 作用域，否则按 User
        let (owner_type, owner_id, task_user_id) = match (conv.agent_id.clone(), conv.user_id.clone()) {
            (Some(agent_id), _) => (
                crate::core::contact::OwnerType::Agent,
                agent_id,
                None,
            ),
            (None, Some(user_id)) => {
                let uid = user_id.clone();
                (
                    crate::core::contact::OwnerType::User,
                    user_id,
                    Some(uid),
                )
            }
            (None, None) => (
                crate::core::contact::OwnerType::User,
                crate::core::owner::DEFAULT_USER_ID.to_string(),
                Some(crate::core::owner::DEFAULT_USER_ID.to_string()),
            ),
        };

        let task = ExtractionTask {
            conversation_id: conv.id.clone(),
            conversation_text: conv.content.clone(),
            owner_type,
            owner_id,
            user_id: task_user_id,
            retry_count: 0,
        };

        queue.push_with_persist(task)?;
        self.dictionary_store
            .update_conversation_status(conversation_id, "pending", None)
            .await?;

        tracing::info!("replay: re-queued conversation {}", conversation_id);
        Ok(())
    }

    /// 批量重放失败的对话提取任务
    ///
    /// 返回 `(成功数, 失败数)`；单条失败不影响其余任务。
    pub async fn replay_failed_conversations(
        &self,
        limit: usize,
    ) -> Result<(usize, usize), ShouxinError> {
        let ids = self
            .dictionary_store
            .list_failed_conversation_ids(limit)
            .await?;

        let mut succeeded = 0usize;
        let mut failed = 0usize;
        for id in &ids {
            match self.replay_conversation(id).await {
                Ok(()) => succeeded += 1,
                Err(e) => {
                    tracing::warn!("replay failed for {}: {}", id, e);
                    failed += 1;
                }
            }
        }
        Ok((succeeded, failed))
    }

    /// 异步添加对话（v0.5.1）
    ///
    /// 立即保存对话到字典库，返回 conversation_id。
    /// LLM 提取在后台异步执行，不阻塞 API 响应。
    pub async fn add_async(
        &self,
        conversation: &str,
        user_id: Option<&str>,
        scope: Scope,
    ) -> Result<AddResult, ShouxinError> {
        let conversation_id = uuid::Uuid::new_v4().to_string();
        let owner_type = self.get_owner_type(&scope);
        let owner_id = match &scope {
            Scope::User => user_id.map(|s| s.to_string()).unwrap_or_else(|| self.config.user_id.clone()),
            Scope::Agent => self.config.agent_id.clone(),
        };

        if let Some(ref queue) = self.background_queue {
            let task = ExtractionTask {
                conversation_id: conversation_id.clone(),
                conversation_text: conversation.to_string(),
                owner_type,
                owner_id: owner_id.clone(),
                user_id: user_id.map(|s| s.to_string()),
                retry_count: 0,
            };
            let task_key = BackgroundTaskQueue::task_key(&conversation_id);
            let task_value = BackgroundTaskQueue::serialize_task(&task)?;

            // 强事务：原文 + 任务在同一个 sled 事务中写入。
            self.dictionary_store
                .save_conversation_with_task(
                    &conversation_id,
                    conversation,
                    chrono::Utc::now(),
                    owner_type,
                    &owner_id,
                    &task_key,
                    &task_value,
                )
                .await?;

            if let Err(e) = queue.push_persisted(task) {
                // 任务已在 sled 中持久化；内存投递失败由重启恢复兜底，不标记 failed。
                tracing::error!(
                    "add_async: conversation {} persisted but channel send failed (will recover on restart): {}",
                    conversation_id, e
                );
            } else {
                tracing::info!(
                    "add_async: conversation {} saved atomically with task ({} chars)",
                    conversation_id,
                    conversation.chars().count()
                );
            }
        } else {
            // 无队列：仍落原文并标记 failed（无法提取）。
            self.dictionary_store.save_conversation(
                &conversation_id,
                conversation,
                chrono::Utc::now(),
                owner_type,
                &owner_id,
            ).await?;
            let reason = "background queue not configured";
            tracing::error!("add_async: {}", reason);
            if let Err(status_err) = self.dictionary_store
                .update_conversation_status(&conversation_id, "failed", Some(reason))
                .await
            {
                tracing::error!(
                    "add_async: failed to mark conversation {} as failed: {}",
                    conversation_id, status_err
                );
            }
        }

        Ok(AddResult {
            conversation_id: Some(conversation_id),
            ..Default::default()
        })
    }

    /// 添加对话（核心方法）
    pub async fn add(
        &self,
        conversation: &str,
        user_id: Option<&str>,
        scope: Scope,
    ) -> Result<AddResult, ShouxinError> {
        let messages = vec![crate::core::Message::new(
            crate::core::message::MessageRole::User,
            conversation.to_string(),
        )?];

        let extracted_elements = match tokio::time::timeout(
            std::time::Duration::from_secs(self.config.extraction_settings.timeout_secs),
            self.extractor.extract(&messages),
        )
        .await
        {
            Ok(Ok(elements)) => elements,
            Ok(Err(e)) => {
                tracing::warn!("Memory extraction failed: {}", e);
                ExtractedElements::new(String::new())
            }
            Err(_elapsed) => {
                tracing::warn!("Memory extraction timed out");
                ExtractedElements::new(String::new())
            }
        };

        let persons = extracted_elements.persons.clone();
        let events = extracted_elements.events.clone();
        let learnings = extracted_elements.learnings.clone();
        let todo_update_intent = extracted_elements.todo_update_intent.clone();

        // 创建唯一的 conversation_id (UUID)
        let conversation_id = uuid::Uuid::new_v4().to_string();
        
        // 获取所有者信息
        let owner_type = self.get_owner_type(&scope);
        let owner_id = match &scope {
            Scope::User => user_id.map(|s| s.to_string()).unwrap_or_else(|| self.config.user_id.clone()),
            Scope::Agent => self.config.agent_id.clone(),
        };

        // 保存原始对话到字典库（使用我们自己的 ID）
        // 需要将所有者信息传入，但trait方法不支持owner参数
        // 直接通过底层字典库的save_conversation方法
        self.dictionary_store.save_conversation(
            &conversation_id,
            conversation,
            chrono::Utc::now(),
            owner_type,
            &owner_id,
        ).await?;

        // 手动处理其他数据分流
        // 统一写入层：联系人写入（与异步路径共用同一实现，避免两套语义漂移）
        //
        // 旧实现硬编码 aliases="[]" / related="[]" / is_agent=false 等，会把关系库里
        // 已有的别名、关系标签、虚拟角色标记清空（F-M1 的同步路径版本）。
        let writer = crate::memory::MemoryWriter::new(
            self.dictionary_store.clone(),
            self.relational_store.clone(),
        );
        let contact_outcome = writer.write_contacts(&persons, owner_type, &owner_id).await?;
        let person_id_to_contact_id = contact_outcome.id_map;


        // 保存事件到关系库
        // P0-4: 将 LLM 临时 person_id 映射为去重后的 final_contact_id
        let map_contact_ids = |raw_ids: &[String]| -> Vec<String> {
            raw_ids.iter()
                .map(|pid| person_id_to_contact_id.get(pid).cloned().unwrap_or_else(|| pid.clone()))
                .collect()
        };
        let event_details: Vec<EventDetail> = events.iter().map(|e| {
            // 解析时间信息（如果有的话）
            let event_time = match e.time_info.as_deref() {
                Some(ti) if !ti.is_empty() => {
                    // 尝试解析时间，失败则使用当前时间
                    DateTime::parse_from_rfc3339(ti)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now())
                }
                _ => Utc::now(),
            };

            // 创建事件索引并保存
            // P0-4: 使用映射后的 final_contact_id
            let mapped_contact_ids = map_contact_ids(&e.related_person_ids);
            let contact_ids_json = serde_json::to_string(&mapped_contact_ids).unwrap_or("[]".to_string());
            let event_parsed_time = parse_event_time(&e.description, Utc::now());
            let mut event_index = crate::core::EventIndex {
                event_id: e.event_id.clone(),
                contact_ids: contact_ids_json,
                owner_id: owner_id.clone(),
                owner_type,
                time_range_start: event_time,
                time_range_end: event_time,
                importance: if e.importance >= 0.9 {
                "high".to_string()
            } else if e.importance > 0.7 {
                "long_term".to_string()
            } else if e.importance >= 0.5 {
                "medium".to_string()
            } else {
                "low".to_string()
            },
                status: "pending".to_string(),
                created_at: Utc::now(),
                completed_at: None,
                summary: e.description.clone(),
                title: e.title.clone(),
                event_type: String::new(),
                deadline: None,
                auto_invalid_at: None,
                is_auto_invalid: false,
                is_merged: false,
                merge_target_id: None,
                last_merge_check_at: None,
                event_time: event_parsed_time,
                importance_score: e.importance as f32,
                is_archived: false,
                archived_at: None,
            };
            event_index.compute_auto_invalid_at();

            // 注意：这里我们不能直接await，因为在map闭包中，所以我们之后处理
            
            EventDetail {
                event_id: e.event_id.clone(),
                title: e.title.clone(),
                description: e.description.clone(),
                time_info: e.time_info.clone(),
                importance: if e.importance >= 0.8 { "high".to_string() } else if e.importance >= 0.5 { "medium".to_string() } else { "low".to_string() },
                status: "pending".to_string(),
                related_contacts: mapped_contact_ids,
                created_at: Utc::now(),
                completed_at: None,
            }
        }).collect();

        // 保存事件索引到关系库（在map之后单独处理）
        for e in &events {
            // V1: 使用对话文本前200字符作为去重键，避免 LLM 非确定性导致同一对话产生不同摘要
            let dedup_key: String = conversation.chars().take(200).collect();

            // D2: 按摘要 + 所有者查重，避免相同事件重复插入
            let owner_type_str = format!("{:?}", owner_type);
            let existing_events = self.relational_store
                .search_events_by_summary(&dedup_key, &owner_type_str, &owner_id)
                .await?;
            if !existing_events.is_empty() {
                tracing::debug!(
                    "事件已存在，跳过插入: {} (existing event_id={})",
                    dedup_key.chars().take(50).collect::<String>(),
                    existing_events[0].event_id
                );
                continue;
            }

            // ========== 语义合并检查 (v0.5.1) ==========
            let event_summary_for_merge = format!(
                "{} {}",
                e.title.as_deref().unwrap_or(""),
                e.description.as_str()
            );

            if let Some(vector_store) = &self.embedded_vector_store {
                match self.embedder.embed(&event_summary_for_merge).await {
                    Ok(new_embedding) => {
                        match vector_store.semantic_search_with_embedding(
                            &new_embedding,
                            3,
                            Some(owner_type),
                            Some(&owner_id),
                        ).await {
                            Ok(similar_results) => {
                                if let Some(best_match) = similar_results.first() {
                                    let similarity = best_match.score;
                                    tracing::debug!(
                                        "[Event Merge] 检查事件 '{}' (相似度: {:.4})",
                                        event_summary_for_merge.chars().take(30).collect::<String>(),
                                        similarity
                                    );

                                    if similarity > 0.90 {
                                        // 自动合并到已有事件
                                        tracing::info!(
                                            "[Event Merge] 自动合并: '{}' -> '{}' (相似度: {:.4})",
                                            event_summary_for_merge.chars().take(30).collect::<String>(),
                                            best_match.material.content.chars().take(30).collect::<String>(),
                                            similarity
                                        );

                                        if let Err(err) = self.relational_store.update_event_after_merge(
                                            &best_match.material.id.to_string(),
                                            &map_contact_ids(&e.related_person_ids),
                                            Some(&event_summary_for_merge),
                                        ).await {
                                            tracing::warn!("[Event Merge] 合并失败: {}", err);
                                        }
                                        continue;
                                    } else if similarity > 0.75 {
                                        // 存入待确认合并区
                                        tracing::info!(
                                            "[Event Merge] 待确认: '{}' 相似度 {:.4} (0.75-0.90)",
                                            event_summary_for_merge.chars().take(30).collect::<String>(),
                                            similarity
                                        );

                                        let source_event_id = format!("pending_{}", uuid::Uuid::new_v4());
                                        if let Err(err) = self.relational_store.insert_pending_event_merge(
                                            &source_event_id,
                                            Some(&best_match.material.id.to_string()),
                                            &event_summary_for_merge,
                                            Some(&best_match.material.content),
                                            similarity,
                                            owner_type,
                                            &owner_id,
                                        ).await {
                                            tracing::warn!("[Event Merge] 插入待确认失败: {}", err);
                                        }
                                        continue;
                                    } else {
                                        tracing::debug!(
                                            "[Event Merge] 相似度 {:.4} < 0.75，创建新事件",
                                            similarity
                                        );
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::warn!("[Event Merge] 向量搜索失败: {}", err);
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!("[Event Merge] 生成 embedding 失败: {}", err);
                    }
                }
            }

            // LLM 提取的描述用于存储（展示用途），存储到 event_type 列
            let llm_description = if e.description.trim().is_empty() {
                String::new()
            } else {
                e.description.clone()
            };

            let event_time = match e.time_info.as_deref() {
                Some(ti) if !ti.is_empty() => {
                    DateTime::parse_from_rfc3339(ti)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now())
                }
                _ => Utc::now(),
            };
            let contact_ids_json = serde_json::to_string(&map_contact_ids(&e.related_person_ids)).unwrap_or("[]".to_string());
            let event_parsed_time = parse_event_time(&e.description, Utc::now());
            let mut event_index = crate::core::EventIndex {
                event_id: e.event_id.clone(),
                contact_ids: contact_ids_json,
                owner_id: owner_id.clone(),
                owner_type,
                time_range_start: event_time,
                time_range_end: event_time,
                importance: if e.importance >= 0.8 {
                "high".to_string()
            } else if e.importance >= 0.5 {
                "medium".to_string()
            } else {
                "low".to_string()
            },
                status: "pending".to_string(),
                created_at: Utc::now(),
                completed_at: None,
                summary: dedup_key.clone(),
                title: e.title.clone(),
                event_type: llm_description,
                deadline: None,
                auto_invalid_at: None,
                is_auto_invalid: false,
                is_merged: false,
                merge_target_id: None,
                last_merge_check_at: None,
                event_time: event_parsed_time,
                importance_score: e.importance as f32,
                is_archived: false,
                archived_at: None,
            };
            event_index.compute_auto_invalid_at();
            self.relational_store.insert_event_index(&event_index).await?;

            // Check hot table capacity
            let is_long_term = e.importance > 0.7;
            let hot_limit = if is_long_term {
                self.config.event_settings.long_term_hot_limit
            } else {
                self.config.event_settings.short_term_hot_limit
            };
            let owner_type_str = format!("{:?}", owner_type);
            self.relational_store.archive_oldest_hot_event(
                &owner_type_str,
                &owner_id,
                is_long_term,
                hot_limit,
            ).await?;

            // 将事件写入向量库
            if let Some(vector_store) = &self.embedded_vector_store {
                if let Ok(embedding) = self.embedder.embed(&event_summary_for_merge).await {
                    let event_content = format!("事件: {} - {}", 
                        e.title.as_deref().unwrap_or("无标题"),
                        e.description.as_str()
                    );
                    let _ = vector_store.insert_material_with_embedding(
                        &event_content,
                        e.title.as_deref(),
                        Some(&e.event_id),
                        "event",
                        &embedding,
                        owner_type,
                        &owner_id,
                    ).await;
                }
            }
        }

        // ========== 处理待办更新意图 ==========
        if let Some(ref intent) = todo_update_intent {
            if let Some(ref title_hint) = intent.title_hint {
                let owner_type_str = format!("{:?}", owner_type);
                match self.relational_store.get_todo_store().search_pending_todos_by_title_fuzzy(
                    title_hint,
                    &owner_type_str,
                    &owner_id,
                ).await {
                    Ok(todos) => {
                        if todos.len() == 1 {
                            let todo = &todos[0];
                            if let Some(ref new_deadline) = intent.new_deadline {
                                match self.relational_store.get_todo_store().update_todo_deadline(
                                    &todo.todo_id,
                                    new_deadline,
                                ).await {
                                    Ok(()) => {
                                        tracing::info!(
                                            "Updated todo '{}' deadline to {} based on conversation intent",
                                            todo.title,
                                            new_deadline
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to update todo deadline: {}", e);
                                    }
                                }
                            }
                        } else if todos.is_empty() {
                            tracing::warn!(
                                "Todo update intent: no pending todo matching '{}'",
                                title_hint
                            );
                        } else {
                            tracing::warn!(
                                "Todo update intent: multiple ({}) pending todos match '{}', skipping",
                                todos.len(),
                                title_hint
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Todo fuzzy search failed: {}", e);
                    }
                }
            }
        }

        let contacts: Vec<ContactDetail> = persons.iter().map(|p| {
            ContactDetail {
                contact_id: p.contact_id.clone(),
                name: p.name.clone(),
                aliases: p.aliases.clone(),
                background: None,
                last_interaction_at: Utc::now(),
                activity_score: 0.0,
                related_contacts: Vec::new(),
                relationship_tags: Vec::new(),
            }
        }).collect();

        let learning_details: Vec<LearningMaterial> = learnings.iter().enumerate().map(|(index, l)| {
            LearningMaterial {
                id: index as u64,
                content: l.content.clone(),
                summary: None,
                source_conversation_id: Some(conversation_id.clone()),
                created_at: Utc::now(),
                material_type: "learning".to_string(),
                canonical_entity_id: None,
                keywords: l.tags.clone(),
                context_before_id: None,
                context_after_id: None,
                triples: Vec::new(),
                owner_type,
                owner_id: owner_id.clone(),
            }
        }).collect();

        let result = AddResult {
            contacts,
            events: event_details,
            learnings: learning_details,
            conversation_id: Some(conversation_id.clone()),
        };

        // ========== 向量库写入：仅写入高价值学习素材 ==========
        // 将提取出的学习素材写入向量库（带重复过滤和质量阈值）
        for learning in learnings.iter() {
            if learning.quality_score < 0.8 {
                tracing::debug!("跳过低质量学习素材 (score={}): {}", learning.quality_score, learning.content);
                continue;
            }
            if let Some(vector_store) = &self.embedded_vector_store {
                // 重复检测：搜索向量库中最相似的素材
                let is_duplicate = if let Ok(embedding) = self.embedder.embed(&learning.content).await {
                    if let Ok(existing) = vector_store.semantic_search_with_embedding(
                        &embedding, 1, Some(owner_type), Some(&owner_id),
                    ).await {
                        !existing.is_empty() && existing[0].score > 0.98
                    } else {
                        false
                    }
                } else {
                    false
                };

                if is_duplicate {
                    tracing::debug!("跳过重复学习素材: {}", learning.content);
                    continue;
                }

                let embedding = self.embedder.embed(&learning.content).await.map_err(|e| ShouxinError::StorageError(format!("Failed to embed learning: {}", e)))?;
                let mat_id = vector_store.insert_material_with_embedding(
                    &learning.content,
                    None,
                    Some(&conversation_id),
                    "learning",
                    &embedding,
                    owner_type,
                    &owner_id,
                ).await.map_err(|e| ShouxinError::StorageError(format!("Failed to add learning to vector store: {}", e)))?;
                tracing::debug!("学习素材已写入向量库，ID: {}", mat_id);
            }
        }
        // ====================================

        Ok(result)
    }

    /// 搜索联系人
    pub async fn search_contacts(
        &self,
        query: &str,
        limit: usize,
        scope: Scope,
        user_id: Option<&str>,
        include_archived: bool,
    ) -> Result<Vec<ContactDetail>, ShouxinError> {
        // 获取所有者信息：scope=User 时使用传入的 user_id
        let owner_type = self.get_owner_type(&scope);
        let owner_id = match &scope {
            Scope::User => user_id.map(|s| s.to_string()).unwrap_or_else(|| self.config.user_id.clone()),
            Scope::Agent => self.config.agent_id.clone(),
        };
        
        // 从关系库搜索联系人索引（使用 owner 过滤）
        let trait_contacts = self.relational_store.search_contacts_with_owner(query, owner_type, &owner_id, limit, include_archived).await?;

        // 补齐画像字段：别名 / 画像 / 关系标签 / 关联联系人
        //
        // 基础检索只取了 id/name/时间，这几列的写入方是"抽取链路 + 心跳画像"，
        // 之前这里是硬编码的空值，导致接口永远返回 aliases=[]、background=null，
        // 看起来像"没有自动生成画像"，实际是读不出来。
        let ids: Vec<String> = trait_contacts.iter().map(|c| c.contact_id.clone()).collect();
        let profiles = self.relational_store.fetch_contact_profiles(&ids).await.unwrap_or_default();

        // 关联联系人只存 ID，这里批量解析成姓名，便于直接展示
        let mut all_related_ids: Vec<String> = Vec::new();
        for p in profiles.values() {
            if let Some(raw) = &p.related_contact_ids {
                all_related_ids.extend(
                    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default(),
                );
            }
        }
        all_related_ids.sort();
        all_related_ids.dedup();
        let names = self
            .relational_store
            .fetch_contact_names(&all_related_ids)
            .await
            .unwrap_or_default();

        let contacts: Vec<ContactDetail> = trait_contacts
            .into_iter()
            .map(|trait_contact| {
                let profile = profiles.get(&trait_contact.contact_id);
                let aliases: Vec<String> = profile
                    .and_then(|p| p.aliases.as_ref())
                    .and_then(|raw| serde_json::from_str(raw).ok())
                    .unwrap_or_default();
                let relationship_tags: Vec<String> = profile
                    .and_then(|p| p.relationship_tags.as_ref())
                    .and_then(|raw| serde_json::from_str(raw).ok())
                    .unwrap_or_default();
                let related_contacts: Vec<String> = profile
                    .and_then(|p| p.related_contact_ids.as_ref())
                    .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|id| names.get(&id).cloned())
                    .collect();

                ContactDetail {
                    contact_id: trait_contact.contact_id.clone(),
                    name: trait_contact.name.clone(),
                    aliases,
                    background: profile.and_then(|p| p.background.clone()),
                    last_interaction_at: trait_contact.last_activity,
                    activity_score: trait_contact.activity_score,
                    related_contacts,
                    relationship_tags,
                }
            })
            .collect();

        Ok(contacts)
    }

    /// 搜索事件
    pub async fn search_events(&self, filter: EventFilter, scope: Scope, user_id: Option<&str>) -> Result<Vec<EventDetail>, ShouxinError> {
        // 获取所有者信息：scope=User 时使用传入的 user_id
        let owner_type = self.get_owner_type(&scope);
        let owner_id = match &scope {
            Scope::User => user_id.map(|s| s.to_string()).unwrap_or_else(|| self.config.user_id.clone()),
            Scope::Agent => self.config.agent_id.clone(),
        };
        
        // 从关系库搜索事件（使用 owner 过滤）
        let trait_events = self.relational_store.search_events_with_owner(&filter, owner_type, &owner_id).await?;
        
        // 转换为 EventDetail
        let events: Vec<EventDetail> = trait_events.into_iter().map(|trait_event| {
            EventDetail {
                event_id: trait_event.event_id.clone(),
                title: trait_event.title.clone(),
                description: trait_event.event_type.clone(),
                time_info: Some(trait_event.event_time.to_rfc3339()),
                importance: if trait_event.importance >= 0.8 { "high".to_string() } else if trait_event.importance >= 0.5 { "medium".to_string() } else { "low".to_string() },
                status: "pending".to_string(),
                related_contacts: vec![],
                created_at: trait_event.created_at,
                completed_at: None,
            }
        }).collect();
        
        Ok(events)
    }

    /// 搜索历史对话
    pub async fn search_history(
        &self,
        time_range: (DateTime<Utc>, DateTime<Utc>),
        scope: Scope,
        user_id: Option<&str>,
    ) -> Result<Vec<Conversation>, ShouxinError> {
        // 获取所有者信息用于过滤
        let owner_type = self.get_owner_type(&scope);
        let owner_id = match &scope {
            Scope::User => user_id.map(|s| s.to_string()).unwrap_or_else(|| self.config.user_id.clone()),
            Scope::Agent => self.config.agent_id.clone(),
        };

        // 从字典库按时间范围获取对话
        let conversations = self.dictionary_store.get_conversation_by_time_range(
            time_range.0,
            time_range.1,
            100,
        ).await?;

        // 在内存中按 owner 过滤（字典库 trait 暂不支持 owner 过滤参数）
        let filtered: Vec<Conversation> = conversations.into_iter()
            .filter(|c| {
                match &scope {
                    Scope::User => c.user_id.as_deref().unwrap_or("default_user") == owner_id,
                    Scope::Agent => c.agent_id.as_deref().unwrap_or("primary_agent_default") == owner_id,
                }
            })
            .collect();

        Ok(filtered)
    }

    /// 语义搜索学习素材
    ///
    /// # 参数
    /// - `query`: 查询文本
    /// - `limit`: 返回结果数量限制
    /// - `scope`: 操作范围
    ///
    /// # 返回
    /// 搜索结果列表
    pub async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        scope: Scope,
        user_id: Option<&str>,
        source_types: Option<Vec<String>>,
    ) -> Result<Vec<VectorSearchResult>, ShouxinError> {
        let query_embedding = self.embedder.embed(query).await?;
        self.semantic_search_with_embedding(&query_embedding, limit, Some(scope), user_id, source_types)
            .await
    }

    /// 将 Memory 转换为 LearningMaterial
    ///
    /// 归属信息从 `Memory.metadata` 读取（写入时由提取/心跳路径写入）。
    /// 原实现硬编码为 `User/default_user`，会让 owner 过滤对所有数据都"匹配"或
    /// 全不匹配，等价于隔离失效（F-VS5）。
    fn memory_to_material(&self, id: &str, memory: &crate::core::Memory) -> LearningMaterial {
        let numeric_id: u64 = id.parse().unwrap_or(0);
        let source_conversation_id = memory.metadata.get("source_conversation_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let owner_type = memory
            .metadata
            .get("owner_type")
            .and_then(|v| v.as_str())
            .and_then(crate::core::contact::OwnerType::parse)
            .unwrap_or(crate::core::contact::OwnerType::User);
        let owner_id = memory
            .metadata
            .get("owner_id")
            .and_then(|v| v.as_str())
            .unwrap_or(crate::core::owner::DEFAULT_USER_ID)
            .to_string();
        let material_type = memory
            .metadata
            .get("material_type")
            .and_then(|v| v.as_str())
            .unwrap_or("knowledge")
            .to_string();
        LearningMaterial {
            id: numeric_id,
            content: memory.content.clone(),
            summary: None,
            source_conversation_id,
            created_at: memory.created_at,
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

    /// 语义搜索学习素材（使用嵌入向量）
    ///
    /// # 参数
    /// - `query_embedding`: 查询嵌入向量
    /// - `limit`: 返回结果数量限制
    /// - `scope`: 操作范围（可选）
    ///
    /// # 返回
    /// 搜索结果列表
    pub async fn semantic_search_with_embedding(
        &self,
        query_embedding: &Embedding,
        limit: usize,
        scope: Option<Scope>,
        user_id: Option<&str>,
        source_types: Option<Vec<String>>,
    ) -> Result<Vec<VectorSearchResult>, ShouxinError> {
        let (filter_owner_type, filter_owner_id) = match scope {
            Some(s) => {
                let ot = self.get_owner_type(&s);
                let oid = match &s {
                    Scope::User => user_id.map(|s| s.to_string()).unwrap_or_else(|| self.config.user_id.clone()),
                    Scope::Agent => self.config.agent_id.clone(),
                };
                (Some(ot), Some(oid))
            },
            None => (None, None),
        };
        
        if let Some(ref store) = self.embedded_vector_store {
            let mut results = store.semantic_search_with_embedding(
                query_embedding,
                limit,
                filter_owner_type,
                filter_owner_id.as_deref(),
            ).await?;

            // F-VS5: 不允许"过滤后为空就退回无过滤"，否则会跨 owner 返回数据。
            // 归属信息缺失属于数据质量问题，应显式暴露而不是放宽边界。
            if results.is_empty() && (filter_owner_type.is_some() || filter_owner_id.is_some()) {
                tracing::debug!(
                    "Semantic search returned 0 results within owner scope {:?}/{:?}",
                    filter_owner_type,
                    filter_owner_id
                );
            }

            if let Some(ref types) = source_types {
                results.retain(|r| types.contains(&r.material.material_type));
            }
            return Ok(results);
        }

        let results = self
            .vector_store
            .search(query_embedding, limit, None)
            .await?;

        let vector_results: Vec<VectorSearchResult> = results
            .into_iter()
            .filter_map(|r| {
                let material = self.memory_to_material(&r.memory.id, &r.memory);
                
                let matches_owner = match (&filter_owner_type, &filter_owner_id) {
                    (Some(ot), Some(oid)) => material.owner_type == *ot && material.owner_id == *oid,
                    (Some(ot), None) => material.owner_type == *ot,
                    (None, Some(oid)) => material.owner_id == *oid,
                    (None, None) => true,
                };
                
                if matches_owner {
                    Some(VectorSearchResult::new(material, r.score))
                } else {
                    None
                }
            })
            .collect();

        Ok(vector_results)
    }

    /// 获取学习素材详情
    pub async fn get_learning_material(
        &self,
        id: &str,
    ) -> Result<Option<LearningMaterial>, ShouxinError> {
        if let Some(ref store) = self.embedded_vector_store {
            return store.get_material(id).await;
        }

        let memory = self.vector_store.get(id).await?;
        Ok(memory.map(|m| self.memory_to_material(id, &m)))
    }

    /// 获取向量库中的素材数量
    pub async fn count_learning_materials(&self) -> Result<usize, ShouxinError> {
        if let Some(ref store) = self.embedded_vector_store {
            return store.count_materials().await;
        }

        self.vector_store.count(None).await
    }

    /// 列出所有学习素材
    pub async fn list_learning_materials(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<LearningMaterial>, ShouxinError> {
        if let Some(ref store) = self.embedded_vector_store {
            return store.list_materials(limit, None, None).await;
        }

        let memories = self.vector_store.list(None).await?;
        let materials: Vec<LearningMaterial> = memories
            .into_iter()
            .take(limit.unwrap_or(usize::MAX))
            .map(|m| self.memory_to_material(&m.id, &m))
            .collect();

        Ok(materials)
    }

    /// 清空向量库
    pub async fn clear_vector_store(&self) -> Result<(), ShouxinError> {
        if let Some(ref store) = self.embedded_vector_store {
            return store.clear_materials().await;
        }

        self.vector_store.clear().await
    }

    /// 获取关系库引用
    pub fn get_relational_store(&self) -> Arc<RelationalStore> {
        self.relational_store.clone()
    }

    /// 获取字典库引用 (v0.5.1)
    pub fn get_dictionary_store(&self) -> Arc<dyn crate::traits::store_traits::DictionaryStoreTrait> {
        self.dictionary_store.clone()
    }

    /// 获取待办事项存储引用
    pub fn get_todo_store(&self) -> TodoListStore {
        self.relational_store.get_todo_store()
    }

    /// 获取向量库实例
    pub fn get_vector_store(&self) -> Arc<dyn VectorStore> {
        self.vector_store.clone()
    }

    /// 获取嵌入式向量库实例
    pub fn get_embedded_vector_store(&self) -> Option<Arc<AnyVectorStore>> {
        self.embedded_vector_store.clone()
    }

    /// 获取待确认事件合并列表 (v0.5.1)
    pub async fn get_pending_event_merges(
        &self,
        owner_type: Option<crate::core::contact::OwnerType>,
        owner_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<crate::stores::relational::PendingEventMerge>, ShouxinError> {
        self.relational_store.get_pending_event_merges(owner_type, owner_id, limit).await
    }

    /// 确认事件合并 (v0.5.1)
    /// 
    /// # 参数
    /// - `merge_id`: 待确认合并记录的 ID
    /// - `action`: 操作类型，"merge" 或 "create"
    /// - `source_event_summary`: 源事件摘要（当 action=create 时使用）
    /// 
    /// # 返回
    /// 创建的事件 ID
    pub async fn confirm_event_merge(
        &self,
        merge_id: i64,
        action: &str,
        source_event_summary: Option<&str>,
    ) -> Result<String, ShouxinError> {
        self.relational_store.confirm_pending_event_merge(merge_id, action, source_event_summary).await
    }
}