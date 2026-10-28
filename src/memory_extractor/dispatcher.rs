//! 数据分流器
//!
//! 将提取的四要素分流到对应的存储库

use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use crate::core::Memory;
use crate::core::Message;
use crate::core::{ContactIndex, ContactTotal, EventIndex as CoreEventIndex};
use crate::error::ShouxinError;
use crate::learning::{EntityNormalizer, Summarizer};
use crate::memory_extractor::extractor::ExtractedElements;
use crate::stores::dictionary::store::{ContactEventRecord, ConversationRecord, DictionaryStore};
use crate::stores::relational::store::RelationalStore;
use crate::traits::Embedder;
use crate::traits::LlmClient;
use crate::traits::VectorStore;

/// 分流输入
#[derive(Debug, Clone)]
pub struct DispatchInput {
    /// 原始对话
    pub conversation: Vec<Message>,
    /// 提取的四要素
    pub extracted_elements: ExtractedElements,
    /// 用户 ID（可选）
    pub user_id: Option<String>,
    /// Agent ID（可选）
    pub agent_id: Option<String>,
}

/// 活跃联系人数量限制
const MAX_ACTIVE_CONTACTS: usize = 50;

/// 数据分流器
pub struct DataDispatcher {
    /// 字典库
    dictionary_store: Arc<DictionaryStore>,
    /// 关系库
    relational_store: Arc<RelationalStore>,
    /// 向量库
    vector_store: Arc<dyn VectorStore>,
    /// 嵌入器
    embedder: Option<Arc<dyn Embedder>>,
    /// LLM 客户端
    llm_client: Option<Arc<dyn LlmClient>>,
}

impl DataDispatcher {
    /// 创建新的数据分流器
    pub fn new(
        dictionary_store: Arc<DictionaryStore>,
        relational_store: Arc<RelationalStore>,
        vector_store: Arc<dyn VectorStore>,
    ) -> Self {
        Self {
            dictionary_store,
            relational_store,
            vector_store,
            embedder: None,
            llm_client: None,
        }
    }

    /// 设置向量库
    pub fn with_vector_store(mut self, vector_store: Arc<dyn VectorStore>) -> Self {
        self.vector_store = vector_store;
        self
    }

    /// 设置嵌入器
    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// 设置 LLM 客户端
    pub fn with_llm_client(mut self, llm_client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(llm_client);
        self
    }

    /// 执行数据分流
    ///
    /// 三分流写入带有错误处理与补偿机制：
    /// - 分流 2（关系库）写入失败时，记录错误并尝试回滚已写入的字典库数据，返回聚合错误。
    /// - 分流 3（向量库）写入失败时，仅记录错误并返回，已写入的字典库和关系库数据保留（向量数据可重新生成）。
    pub async fn dispatch(&self, input: &DispatchInput) -> Result<(), ShouxinError> {
        let elements = &input.extracted_elements;

        // 分流 1：全量数据写入字典库
        let conversation_id = self.dispatch_to_dictionary(input, elements).await?;

        // 分流 2：元数据写入关系库
        if let Err(rel_err) = self.dispatch_to_relational(input, elements).await {
            tracing::error!(
                error = %rel_err,
                conversation_id = %conversation_id,
                "分流 2 关系库写入失败，尝试回滚已写入的字典库数据"
            );
            // 尝试回滚字典库数据（字典库暂无删除接口，记录待清理标识以便后续补偿）
            if let Err(rollback_err) = self.rollback_dictionary(&conversation_id, elements).await {
                tracing::error!(
                    error = %rollback_err,
                    conversation_id = %conversation_id,
                    "字典库回滚失败，需人工介入清理"
                );
            }
            return Err(ShouxinError::GeneralError(format!(
                "分流 2 关系库写入失败，已尝试回滚字典库 (conversation_id={}): {}",
                conversation_id, rel_err
            )));
        }

        // 分流 3：学习素材写入向量库
        if let Err(vec_err) = self.dispatch_to_vector(elements, &conversation_id).await {
            tracing::error!(
                error = %vec_err,
                conversation_id = %conversation_id,
                "分流 3 向量库写入失败，已写入的字典库和关系库数据保留（向量数据可重新生成）"
            );
            return Err(ShouxinError::GeneralError(format!(
                "分流 3 向量库写入失败 (conversation_id={}): {}",
                conversation_id, vec_err
            )));
        }

        Ok(())
    }

    /// 回滚字典库中已写入的数据
    ///
    /// 当关系库写入失败时调用。字典库目前未提供按 conversation_id 删除的接口，
    /// 此处通过日志记录待清理的 conversation_id 与关联数据，便于后续补偿处理。
    async fn rollback_dictionary(
        &self,
        conversation_id: &str,
        elements: &ExtractedElements,
    ) -> Result<(), ShouxinError> {
        tracing::warn!(
            conversation_id = %conversation_id,
            contact_count = elements.persons.len(),
            event_count = elements.events.len(),
            "字典库回滚：记录待清理的字典库数据（conversation_id 及关联联系人和事件）"
        );
        Ok(())
    }

    /// 分流 1：全量数据写入字典库
    async fn dispatch_to_dictionary(
        &self,
        _input: &DispatchInput,
        elements: &ExtractedElements,
    ) -> Result<String, ShouxinError> {
        // 写入对话原文
        let conversation_id = Uuid::new_v4().to_string();
        let conversation_record = ConversationRecord::new(
            conversation_id.clone(),
            elements.conversation_text.clone(),
            Utc::now(),
        );
        self.dictionary_store
            .save_conversation(
                &conversation_record.id,
                &conversation_record.content,
                conversation_record.timestamp,
                crate::core::contact::OwnerType::User,
                "default_user",
            )
            .await?;

        // 写入联系人信息
        for person in &elements.persons {
            let contact_result: Option<crate::core::ContactTotal> = self
                    .dictionary_store
                    .get_contact(&person.contact_id)
                    .await?;
                let mut contact: crate::core::ContactTotal = contact_result
                .unwrap_or_else(|| {
                    ContactTotal::new(person.contact_id.clone(), person.name.clone())
                });

            // 更新最后交互时间
            contact.update_interaction();

            // 保存到字典库
            self.dictionary_store.save_contact(&contact).await?;
        }

        // 写入联系人历史事件记录（全量事件摘要）
        for event in &elements.events {
            // 为每个关联的联系人写入历史事件记录
            for contact_id in &event.related_person_ids {
                // 确定重要性级别
                let importance = Self::classify_importance(event.importance);

                // 分析用户情感倾向（简化实现，实际应由 LLM 分析）
                let user_sentiment = Self::analyze_sentiment(&event.description);

                // 创建联系人历史事件记录
                let contact_event_record = ContactEventRecord::new(
                    event.event_id.clone(),
                    contact_id.clone(),
                    "__self__".to_string(),
                    crate::core::contact::OwnerType::User,
                    event.description.clone(),
                    Utc::now(),
                    importance,
                    user_sentiment,
                    Some(conversation_id.clone()),
                );

                // 保存到字典库
                self.dictionary_store
                    .save_contact_event(&contact_event_record)
                    .await?;
            }
        }

        Ok(conversation_id)
    }

    /// 分流 2：元数据写入关系库
    async fn dispatch_to_relational(
        &self,
        _input: &DispatchInput,
        elements: &ExtractedElements,
    ) -> Result<(), ShouxinError> {
        // 写入联系人索引
        for person in &elements.persons {
            let contact_total_result: Option<crate::core::ContactTotal> = self
                    .dictionary_store
                    .get_contact(&person.contact_id)
                    .await?;
                let contact_total: crate::core::ContactTotal = contact_total_result
                .unwrap_or_else(|| {
                    ContactTotal::new(person.contact_id.clone(), person.name.clone())
                });

            let contact_index = ContactIndex::from_total(&contact_total);
            self.relational_store
                .upsert_contact_index(&contact_index)
                .await?;

            // 写入别名索引
            if !person.aliases.is_empty() {
                self.relational_store
                    .insert_contact_aliases(&contact_index.id, &person.aliases, contact_index.owner_type, &contact_index.owner_id)
                    .await?;
            }
        }

        // 写入事件索引（仅长期和重大事件）
        for event in &elements.events {
            let importance = Self::classify_importance(event.importance);

            // 只有长期和重大事件才写入关系库索引表
            if importance == "long_term" || importance == "critical" {
                let contact_ids: Vec<String> = event.related_person_ids.clone();
                let core_event_index = CoreEventIndex::new(
                    event.event_id.clone(),
                    contact_ids,
                    event.description.clone(),
                );

                let core_event_index = core_event_index.with_importance(importance);
                self.relational_store
                    .insert_event_index(&core_event_index)
                    .await?;
            }
            // 短期事件不写入关系库索引表，仅留存于字典库联系人历史中
        }

        // 管理活跃联系人
        self.manage_active_contacts().await?;

        Ok(())
    }

    /// 分流 3：学习素材写入向量库（经过实体归一化和摘要处理）
    async fn dispatch_to_vector(
        &self,
        elements: &ExtractedElements,
        conversation_id: &str,
    ) -> Result<(), ShouxinError> {
        // 检查嵌入器是否存在
        if let Some(embedder) = &self.embedder {
            // 遍历提取的学习素材
            for learning in &elements.learnings {
                // 只有高质量学习素材才写入向量库
                if learning.quality_score >= 0.8 {
                    // 为学习素材生成嵌入向量
                    let embedding = embedder.embed(&learning.content).await?;

                    let mut memory = Memory::new(learning.content.clone())?
                        .with_embedding(embedding)
                        .with_metadata(
                            "quality_score",
                            serde_json::Value::Number(
                                serde_json::Number::from_f64(learning.quality_score as f64)
                                    .unwrap_or_else(|| serde_json::Number::from(0)),
                            ),
                        )
                        .with_metadata("source", serde_json::Value::String("extracted".to_string()))
                        .with_metadata(
                            "conversation_id",
                            serde_json::Value::String(conversation_id.to_string()),
                        );

                    // 如果有 LLM 客户端，进行实体归一化和摘要处理
                    if let Some(llm_client) = &self.llm_client {
                        // 实体归一化
                        let normalizer = EntityNormalizer::new(llm_client.clone(), 100);
                        if let Ok(entities) = normalizer.normalize(&learning.content).await {
                            if let Some(first_entity) = entities.first() {
                                memory = memory.with_metadata(
                                    "canonical_entity_id",
                                    serde_json::Value::String(first_entity.canonical.clone()),
                                );
                            }
                        }

                        // 摘要和关键词生成
                        let summarizer = Summarizer::new(llm_client.clone());
                        if let Ok(summary_result) = summarizer.summarize(&learning.content).await {
                            memory = memory.with_metadata(
                                "summary",
                                serde_json::Value::String(summary_result.summary),
                            );
                            memory = memory.with_metadata(
                                "keywords",
                                serde_json::Value::Array(
                                    summary_result
                                        .keywords
                                        .into_iter()
                                        .map(serde_json::Value::String)
                                        .collect(),
                                ),
                            );
                        }
                    }

                    // 插入到向量库
                    self.vector_store.insert(memory).await?;
                }
            }
        }

        Ok(())
    }

    /// 管理活跃联系人
    async fn manage_active_contacts(&self) -> Result<(), ShouxinError> {
        // 获取当前活跃联系人数量
        let active_contacts: Vec<crate::core::ContactIndex> = self.relational_store.get_active_contacts(
            crate::core::contact::OwnerType::User,
            "default_user",
            i64::MAX,
        ).await?;

        if active_contacts.len() > MAX_ACTIVE_CONTACTS {
            // 需要删除超出数量的联系人（从关系库删除索引，字典库保留）
            let to_remove = &active_contacts[MAX_ACTIVE_CONTACTS..];
            for contact in to_remove {
                self.relational_store
                    .delete_contact_index(&contact.id)
                    .await?;
            }
        }

        Ok(())
    }

    /// 更新联系人最后交互时间
    pub async fn update_contact_interaction(&self, contact_id: &str) -> Result<(), ShouxinError> {
        // 更新字典库
        let contact_result: Option<crate::core::ContactTotal> = self.dictionary_store.get_contact(contact_id).await?;
        if let Some(mut contact) = contact_result {
            contact.update_interaction();
            self.dictionary_store.save_contact(&contact).await?;

            // 更新关系库
            let contact_index = ContactIndex::from_total(&contact);
            self.relational_store
                .upsert_contact_index(&contact_index)
                .await?;
        }

        Ok(())
    }

    /// 分类事件重要性
    ///
    /// # 参数
    /// - `importance_score`: 重要性分数（0.0-1.0）
    ///
    /// # 返回
    /// 重要性级别（short_term/long_term/critical）
    ///
    /// # 说明
    /// 实现委托给 [`crate::utils::classify_importance`]，与异步写入路径共用同一套阈值。
    fn classify_importance(importance_score: f32) -> String {
        crate::utils::classify_importance(importance_score).to_string()
    }

    /// 分析用户情感倾向（简化实现）
    ///
    /// # 参数
    /// - `description`: 事件描述
    ///
    /// # 返回
    /// 情感倾向（positive/neutral/negative）
    ///
    /// # 说明
    /// 实现委托给 [`crate::utils::analyze_sentiment`]，与异步写入路径共用同一套关键词。
    fn analyze_sentiment(description: &str) -> String {
        crate::utils::analyze_sentiment(description).to_string()
    }
}

/// 数据分流器 trait（向后兼容）
#[async_trait]
pub trait DataDispatcherTrait: Send + Sync {
    async fn dispatch(&self, input: &DispatchInput) -> Result<(), ShouxinError>;
}

#[async_trait]
impl DataDispatcherTrait for DataDispatcher {
    async fn dispatch(&self, input: &DispatchInput) -> Result<(), ShouxinError> {
        self.dispatch(input).await
    }
}
