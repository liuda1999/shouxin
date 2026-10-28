use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::error::ShouxinError;
use crate::heartbeat::analyzer::HeartbeatAnalyzer;
use crate::heartbeat::question_pool::QuestionPool;
use crate::heartbeat::types::{HeartbeatAction, HeartbeatReport};
use crate::memory::any_vector_store::AnyVectorStore;
use crate::stores::relational::store::RelationalStore;

pub struct HeartbeatExecutor {
    analyzer: Arc<HeartbeatAnalyzer>,
    question_pool: Arc<QuestionPool>,
    rel_store: Arc<RelationalStore>,
    vector_store: Option<Arc<AnyVectorStore>>,
    /// 待办生命周期策略（来自 [todo] 配置）
    todo_settings: crate::config::TodoSettings,
}

impl HeartbeatExecutor {
    pub fn new(
        analyzer: Arc<HeartbeatAnalyzer>,
        question_pool: Arc<QuestionPool>,
        rel_store: Arc<RelationalStore>,
    ) -> Self {
        Self {
            analyzer,
            question_pool,
            rel_store,
            vector_store: None,
            todo_settings: crate::config::TodoSettings::default(),
        }
    }

    /// 设置向量库引用（用于孤儿记忆清理）
    pub fn with_vector_store(mut self, vs: Arc<AnyVectorStore>) -> Self {
        self.vector_store = Some(vs);
        self
    }

    /// 注入待办策略（替代 30/50 硬编码）
    pub fn with_todo_settings(mut self, todo_settings: crate::config::TodoSettings) -> Self {
        self.todo_settings = todo_settings;
        self
    }

    pub fn rel_store(&self) -> Arc<RelationalStore> {
        self.rel_store.clone()
    }

    /// 运行完整心跳（维护 + LLM），保持向后兼容
    pub async fn run_heartbeat(&self) -> Result<HeartbeatReport, ShouxinError> {
        let maintenance_report = self.run_maintenance_heartbeat().await?;
        match self.run_llm_heartbeat().await {
            Ok(llm_report) => {
                // 合并报告
                let mut report = llm_report;
                report.duration_ms += maintenance_report.duration_ms;
                Ok(report)
            }
            Err(e) => {
                warn!("LLM heartbeat failed, returning maintenance report only: {:?}", e);
                Ok(maintenance_report)
            }
        }
    }

    /// 仅运行维护心跳（纯SQL任务，永不调用LLM）
    pub async fn run_maintenance_heartbeat(&self) -> Result<HeartbeatReport, ShouxinError> {
        let started_at = Utc::now();
        info!("Starting maintenance heartbeat at {}", started_at);

        let mut stats = ExecutionStats::default();

        // 清理过期的 pending merge
        let _ = self.rel_store.cleanup_expired_pending_merges().await;

        // 记忆提升/降级：必须在 cleanup_expired_events 之前执行，否则被频繁引用的短期记忆会被先归档
        match self.rel_store.promote_demote_events().await {
            Ok((promoted, demoted)) => {
                if promoted > 0 || demoted > 0 {
                    info!("[Lifecycle] Promoted {} events, demoted {} events", promoted, demoted);
                }
            }
            Err(e) => warn!("[Lifecycle] Failed to promote/demote events: {}", e),
        }

        // 清理过期事件（promote 之后执行，已提升的记忆不会被归档）
        let _ = self.rel_store.cleanup_expired_events().await;
        let cleaned = self.rel_store.cleanup_low_importance_events().await.unwrap_or(0);
        stats.events_cleaned += cleaned;

        // 向量库孤儿记忆清理：删除 SQLite 中已不存在的事件对应的向量库记忆
        if let Some(ref vs) = self.vector_store {
            match self.cleanup_orphaned_vector_memories(vs).await {
                Ok(count) => {
                    if count > 0 {
                        info!("[Orphan Cleanup] Removed {} orphaned event memories from vector store", count);
                        stats.events_cleaned += count;
                    }
                }
                Err(e) => warn!("[Orphan Cleanup] Failed to cleanup orphaned vector memories: {}", e),
            }
        }

        // Contact archiving
        match self.rel_store.archive_stale_contacts(1000).await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!("Archived {} stale contacts (not contacted for 180+ days)", count);
                    stats.contacts_updated += count;
                }
            }
            Err(e) => tracing::warn!("Failed to archive stale contacts: {}", e),
        }

        // 待办 deadline 到期失效：将 deadline 已过期的 pending 待办标记为 auto_invalid
        match self.rel_store.invalidate_expired_todos().await {
            Ok(count) => {
                if count > 0 {
                    info!("[Todo Lifecycle] Invalidated {} expired todos (deadline passed)", count);
                    stats.events_cleaned += count;
                }
            }
            Err(e) => warn!("[Todo Lifecycle] Failed to invalidate expired todos: {}", e),
        }

        // 归档已完成/已失效的待办
        let _ = self.rel_store.cleanup_todos().await;

        // Todo hot/cold management（阈值来自 [todo] 配置）
        let todo_policy = self.todo_settings.clone();
        let owners = self.rel_store.get_all_todo_owners().await.unwrap_or_default();
        for (owner_type, owner_id) in &owners {
            let pending_count = self.rel_store.count_pending_todos(owner_type, owner_id).await.unwrap_or(0);

            // Archive FIRST: 超过 max_pending_before_archive 时归档最旧的一批
            // （在 reminder 之前执行，避免 reminder 自己把 count 顶上去）
            if pending_count > todo_policy.max_pending_before_archive as i64 {
                match self.rel_store.archive_stale_todos(
                    owner_type,
                    owner_id,
                    todo_policy.archive_keep_count,
                    todo_policy.archive_after_days,
                ).await {
                    Ok(count) => {
                        if count > 0 {
                            info!("Archived {} stale todos for {}/{}", count, owner_type, owner_id);
                            stats.events_cleaned += count;
                        }
                    }
                    Err(e) => warn!("Failed to archive stale todos: {}", e),
                }
            }

            // Reminder AFTER archive: 使用归档后的最新 count
            let pending_after_archive = self.rel_store.count_pending_todos(owner_type, owner_id).await.unwrap_or(0);
            if pending_after_archive > todo_policy.reminder_threshold as i64 {
                if !self.rel_store.has_recent_reminder(owner_type, owner_id).await.unwrap_or(true) {
                    let reminder_id = uuid::Uuid::new_v4().to_string();
                    let title = format!("⚠️ 你有 {} 个待办未处理，请及时处理", pending_after_archive);
                    if let Err(e) = self.rel_store.insert_reminder_todo(&reminder_id, owner_type, owner_id, &title).await {
                        warn!("Failed to insert reminder todo: {}", e);
                    } else {
                        info!("Generated reminder todo for {}/{}: {} pending", owner_type, owner_id, pending_after_archive);
                        stats.questions_created += 1;
                    }
                }
            }
        }

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        info!(
            "Maintenance heartbeat completed: {} questions, {} contacts updated, {} events cleaned, {} duration_ms",
            stats.questions_created,
            stats.contacts_updated,
            stats.events_cleaned,
            duration_ms
        );

        Ok(HeartbeatReport {
            started_at,
            completed_at,
            actions_generated: 0,
            questions_created: stats.questions_created,
            contacts_updated: stats.contacts_updated,
            events_cleaned: stats.events_cleaned,
            events_merged: stats.events_merged,
            contacts_merged: stats.contacts_merged,
            ambiguities_detected: stats.ambiguities_detected,
            profiles_updated: stats.profiles_updated,
            skipped: false,
            skip_reason: None,
            duration_ms,
            maintenance_only: true,
        })
    }

    /// 运行LLM智能心跳（包含analyzer和action处理）
    pub async fn run_llm_heartbeat(&self) -> Result<HeartbeatReport, ShouxinError> {
        let started_at = Utc::now();
        info!("Starting LLM heartbeat at {}", started_at);

        let processing_state = self.rel_store.get_processing_state().await?;
        let last_processed_ts = processing_state.last_processed_ts;

        let (actions, last_conv_ts) = self.analyzer.process_new_conversations(last_processed_ts).await?;

        let analysis_actions = self.analyzer.analyze().await?;
        let mut all_actions = actions;
        all_actions.extend(analysis_actions);

        let mut stats = ExecutionStats::default();

        for action in &all_actions {
            match action {
                HeartbeatAction::AskUser(question) => {
                    if let Err(e) = self.question_pool.add_question(question.clone()).await {
                        warn!("Failed to add question to pool: {:?}", e);
                    } else {
                        stats.questions_created += 1;
                    }

                    let todo = crate::stores::relational::TodoItem::new(
                        question.id.clone(),
                        question.related_contact_ids.clone(),
                        "default_user".to_string(),
                        crate::core::contact::OwnerType::User,
                        question.title.clone(),
                        question.expires_at.unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::days(3)),
                        "medium".to_string(),
                    )
                    .with_description(question.detail.clone())
                    .with_todo_type("ambiguity".to_string())
                    .with_entity("ambiguity".to_string(), question.id.clone());

                    if let Err(e) = self.rel_store.add_todo(&todo).await {
                        warn!("Failed to add ambiguity todo to store: {:?}", e);
                    }
                }
                HeartbeatAction::UpdateContactSummary { contact_id, new_summary } => {
                    if let Err(e) = self.update_contact_summary(contact_id, new_summary).await {
                        warn!("Failed to update contact summary: {:?}", e);
                    } else {
                        stats.contacts_updated += 1;
                    }
                }
                HeartbeatAction::MarkEventCompleted { event_id, completed_at } => {
                    if let Err(e) = self.mark_event_completed(event_id, *completed_at).await {
                        warn!("Failed to mark event completed: {:?}", e);
                    } else {
                        stats.events_cleaned += 1;
                    }
                }
                HeartbeatAction::MergeEntities { canonical_id, alias_ids } => {
                    if let Err(e) = self.merge_entities(canonical_id, alias_ids).await {
                        warn!("Failed to merge entities: {:?}", e);
                    } else {
                        stats.contacts_merged += 1;
                    }
                }
                HeartbeatAction::CleanupPendingEvents { event_ids } => {
                    if let Err(e) = self.cleanup_events(event_ids).await {
                        warn!("Failed to cleanup events: {:?}", e);
                    } else {
                        stats.events_cleaned += event_ids.len();
                    }
                }
                HeartbeatAction::UpdateTodoStatus { todo_id, status } => {
                    if let Err(e) = self.update_todo_status(todo_id, status).await {
                        warn!("Failed to update todo status: {:?}", e);
                    }
                }
                HeartbeatAction::ResolveAmbiguity { ambiguity_id, answer } => {
                    if let Err(e) = self.resolve_ambiguity(ambiguity_id, answer).await {
                        warn!("Failed to resolve ambiguity: {:?}", e);
                    } else {
                        stats.ambiguities_resolved += 1;
                    }
                }
                HeartbeatAction::NoOp => {}
            }
        }

        self.question_pool.cleanup_expired().await?;

        let completed_at = Utc::now();
        // 游标推进逻辑：
        // 1. 优先使用带时间戳的 action（AskUser.created_at, MarkEventCompleted.completed_at）的最大值
        // 2. 如果没有带时间戳的 action，回退到最后处理的对话时间戳（last_conv_ts）
        // 3. 如果没有新对话（last_conv_ts 为 None），不推进游标，保持原值
        //    避免无处理时游标推进到当前时间，导致历史对话无法回溯
        let max_timestamp = all_actions.iter()
            .filter_map(|a| match a {
                HeartbeatAction::AskUser(q) => Some(q.created_at),
                HeartbeatAction::MarkEventCompleted { completed_at, .. } => Some(*completed_at),
                _ => None,
            })
            .max()
            .or(last_conv_ts);

        if let Some(ts) = max_timestamp {
            self.rel_store.update_processing_state(ts).await?;
        }

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        info!(
            "LLM heartbeat completed: {} actions, {} questions, {} contacts updated, {} events cleaned, {} duration_ms",
            all_actions.len(),
            stats.questions_created,
            stats.contacts_updated,
            stats.events_cleaned,
            duration_ms
        );

        Ok(HeartbeatReport {
            started_at,
            completed_at,
            actions_generated: all_actions.len(),
            questions_created: stats.questions_created,
            contacts_updated: stats.contacts_updated,
            events_cleaned: stats.events_cleaned,
            events_merged: stats.events_merged,
            contacts_merged: stats.contacts_merged,
            ambiguities_detected: stats.ambiguities_detected,
            profiles_updated: stats.profiles_updated,
            skipped: false,
            skip_reason: None,
            duration_ms,
            maintenance_only: false,
        })
    }

    async fn update_contact_summary(&self, contact_id: &str, new_summary: &str) -> Result<(), ShouxinError> {
        self.rel_store.update_contact_background(contact_id, new_summary).await
    }

    async fn mark_event_completed(&self, event_id: &str, completed_at: DateTime<Utc>) -> Result<(), ShouxinError> {
        self.rel_store.update_event_status(event_id, "completed", completed_at).await
    }

    async fn merge_entities(&self, canonical_id: &str, alias_ids: &[String]) -> Result<(), ShouxinError> {
        for alias_id in alias_ids {
            self.rel_store.merge_contacts(canonical_id, alias_id).await?;
            debug!("Merged contact {} into {}", alias_id, canonical_id);
        }
        Ok(())
    }

    async fn cleanup_events(&self, event_ids: &[String]) -> Result<(), ShouxinError> {
        for event_id in event_ids {
            self.rel_store.mark_event_completed(event_id).await?;
            debug!("Cleaned up event {}", event_id);
        }
        Ok(())
    }

    async fn update_todo_status(&self, todo_id: &str, status: &str) -> Result<(), ShouxinError> {
        let (owner_type, owner_id) = match self.rel_store.get_todo_owner(todo_id).await? {
            Some((ot, oi)) => (ot, oi),
            None => {
                warn!("Todo {} not found", todo_id);
                return Ok(());
            }
        };

        match status {
            "completed" => {
                self.rel_store.complete_todo(todo_id, owner_type, &owner_id).await?;
            }
            "dismissed" => {
                self.rel_store.delete_todo(todo_id, owner_type, &owner_id).await?;
            }
            _ => {
                warn!("Unknown todo status: {}", status);
            }
        }

        Ok(())
    }

    async fn resolve_ambiguity(&self, ambiguity_id: &str, answer: &str) -> Result<(), ShouxinError> {
        self.rel_store.resolve_ambiguity(ambiguity_id, answer).await
    }

    /// 清理向量库中的孤儿记忆
    ///
    /// 遍历向量库中所有 material_type="event" 的记忆，
    /// 若其 metadata.event_id 在 SQLite event_index 中不存在，则删除该记忆。
    async fn cleanup_orphaned_vector_memories(
        &self,
        vs: &Arc<AnyVectorStore>,
    ) -> Result<usize, ShouxinError> {
        use crate::traits::vector_store::VectorStore;

        // 获取 SQLite 中所有 event_id
        let valid_event_ids = self.rel_store.get_all_event_ids().await?;

        // 列出向量库中所有记忆（通过解引用 Arc 调用 trait 方法）
        let all_memories = VectorStore::list(&**vs, None).await?;

        let mut orphan_ids: Vec<String> = Vec::new();
        for mem in &all_memories {
            // 只检查 event 类型的记忆
            let material_type = mem
                .metadata
                .get("material_type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if material_type != "event" {
                continue;
            }

            // 检查 event_id 是否在 SQLite 中存在
            let event_id = mem
                .metadata
                .get("event_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if event_id.is_empty() {
                continue;
            }

            if !valid_event_ids.contains(event_id) {
                orphan_ids.push(mem.id.clone());
            }
        }

        let count = orphan_ids.len();
        if count > 0 {
            info!(
                "[Orphan Cleanup] Found {} orphaned event memories (of {} total event memories), deleting...",
                count,
                all_memories.iter().filter(|m| {
                    m.metadata.get("material_type").and_then(|v| v.as_str()).unwrap_or("") == "event"
                }).count()
            );
            for orphan_id in &orphan_ids {
                if let Err(e) = VectorStore::delete(&**vs, orphan_id).await {
                    warn!("[Orphan Cleanup] Failed to delete orphan memory {}: {}", orphan_id, e);
                }
            }
        }

        Ok(count)
    }
}

#[derive(Default)]
struct ExecutionStats {
    questions_created: usize,
    contacts_updated: usize,
    events_cleaned: usize,
    events_merged: usize,
    contacts_merged: usize,
    ambiguities_detected: usize,
    ambiguities_resolved: usize,
    profiles_updated: usize,
}
