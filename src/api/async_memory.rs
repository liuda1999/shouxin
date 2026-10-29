//! 异步记忆管理 API
//!
//! 提供基于三库架构的异步 API
//!
//! # 架构说明
//! 本模块使用 `ThreeLibraryMemoryManager` 作为核心，提供：
//! - 对话添加与四要素提取
//! - 联系人搜索（关系库索引 → 字典库回源）
//! - 事件搜索（关系库索引 → 字典库回源）
//! - 历史对话搜索（字典库时序扫描）
//! - 语义搜索（向量库搜索）
//! - 待办事项管理（关系库）

use chrono::{DateTime, Utc};
use std::sync::Arc;

use crate::core::Memory;
use crate::core::contact::Scope;
use crate::error::ShouxinError;
use crate::memory::three_library_manager::{AddResult, ThreeLibraryMemoryManager};
use crate::stores::relational::TodoItem;
use crate::stores::vector::types::{LearningMaterial, VectorSearchResult};
use crate::traits::store_traits::{ContactDetail, Conversation, EventDetail, EventFilter};

/// 待办事项过滤器
///
/// 用于筛选待办事项
#[derive(Debug, Clone, Default)]
pub struct TodoFilter {
    /// 状态过滤（pending / completed / auto_invalid）
    pub status: Option<String>,
    /// 关联联系人 ID
    pub contact_id: Option<String>,
    /// 结果数量限制
    pub limit: i64,
    /// 是否包含已归档的待办
    pub include_archived: bool,
}

impl TodoFilter {
    /// 创建新的过滤器
    pub fn new() -> Self {
        Self {
            status: None,
            contact_id: None,
            limit: 100,
            include_archived: false,
        }
    }

    /// 设置状态过滤
    pub fn with_status(mut self, status: String) -> Self {
        self.status = Some(status);
        self
    }

    /// 设置联系人过滤
    pub fn with_contact(mut self, contact_id: String) -> Self {
        self.contact_id = Some(contact_id);
        self
    }

    /// 设置结果数量限制
    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = limit;
        self
    }

    /// 设置是否包含已归档
    pub fn with_include_archived(mut self, include_archived: bool) -> Self {
        self.include_archived = include_archived;
        self
    }
}

/// 异步记忆管理 API
///
/// 基于三库架构的统一记忆管理接口
///
/// # 示例
/// ```ignore
/// let memory = AsyncMemory::new(manager);
///
/// // 添加对话
/// let result = memory.add("我是张三，我喜欢编程", Some("user_001")).await?;
///
/// // 搜索联系人
/// let contacts = memory.search_contacts("张三", 10).await?;
///
/// // 添加待办
/// let todo = TodoItem::new(...);
/// memory.add_todo(&todo).await?;
/// ```
pub struct AsyncMemory {
    /// 三库记忆管理器
    manager: Arc<ThreeLibraryMemoryManager>,
    /// 待办生命周期策略（来自 [todo] 配置）
    todo_settings: crate::config::TodoSettings,
}

impl AsyncMemory {
    /// 创建新的 AsyncMemory 实例
    ///
    /// # 参数
    /// - `manager`: 三库记忆管理器实例
    ///
    /// # 返回
    /// AsyncMemory 实例
    pub fn new(manager: Arc<ThreeLibraryMemoryManager>) -> Self {
        Self {
            manager,
            todo_settings: crate::config::TodoSettings::default(),
        }
    }

    /// 注入待办策略（替代 30/50 硬编码）
    pub fn with_todo_settings(mut self, todo_settings: crate::config::TodoSettings) -> Self {
        self.todo_settings = todo_settings;
        self
    }

    /// 重放指定对话的提取任务（失败/中断后恢复）
    pub async fn replay_conversation(&self, conversation_id: &str) -> Result<(), ShouxinError> {
        self.manager.replay_conversation(conversation_id).await
    }

    /// 批量重放失败对话，返回 (成功数, 失败数)
    pub async fn replay_failed_conversations(
        &self,
        limit: usize,
    ) -> Result<(usize, usize), ShouxinError> {
        self.manager.replay_failed_conversations(limit).await
    }

    /// 获取管理器引用 (v0.5.1)
    pub fn get_manager(&self) -> &ThreeLibraryMemoryManager {
        &self.manager
    }

    /// 获取字典存储引用 (v0.5.1)
    pub fn get_dictionary_store(&self) -> Arc<dyn crate::traits::store_traits::DictionaryStoreTrait> {
        self.manager.get_dictionary_store()
    }

    /// 获取关系存储引用 (v0.5.1)
    pub fn get_relational_store(&self) -> Arc<crate::stores::relational::RelationalStore> {
        self.manager.get_relational_store()
    }

    /// 添加对话（核心方法）
    ///
    /// 完整的对话添加流程：
    /// 1. 使用四要素提取器从对话中提取人物、事件、学习素材
    /// 2. 调用数据分流器将提取结果分流到三库
    /// 3. 返回添加结果
    ///
    /// # 参数
    /// - `conversation`: 对话文本内容
    /// - `user_id`: 可选的用户 ID
    /// - `scope`: 操作范围，指定是操作用户数据还是智能体数据
    ///
    /// # 返回
    /// 添加结果，包含新增的联系人、事件和学习素材
    ///
    /// # 示例
    /// ```ignore
    /// let result = memory.add(
    ///     "我是张三，明天要去北京出差",
    ///     Some("user_001"),
    ///     Scope::User
    /// ).await?;
    ///
    /// println!("新增联系人: {}", result.contacts.len());
    /// println!("新增事件: {}", result.events.len());
    /// ```
    pub async fn add(
        &self,
        conversation: &str,
        user_id: Option<&str>,
        scope: Scope,
    ) -> Result<AddResult, ShouxinError> {
        self.manager.add(conversation, user_id, scope).await
    }

    /// 异步添加对话（v0.5.1）
    ///
    /// 立即返回，不等待 LLM 提取完成。
    /// LLM 提取在后台异步执行。
    pub async fn add_async(
        &self,
        conversation: &str,
        user_id: Option<&str>,
        scope: Scope,
    ) -> Result<AddResult, ShouxinError> {
        self.manager.add_async(conversation, user_id, scope).await
    }

    /// 搜索联系人
    ///
    /// 搜索流程：
    /// 1. 先查关系库联系人索引表（快速检索）
    /// 2. 回源字典库获取完整内容
    ///
    /// # 参数
    /// - `query`: 搜索查询（联系人名称或 ID）
    /// - `limit`: 返回结果数量限制
    /// - `scope`: 操作范围，指定是操作用户数据还是智能体数据
    ///
    /// # 返回
    /// 联系人详情列表
    ///
    /// # 示例
    /// ```ignore
    /// let contacts = memory.search_contacts("张三", 10, Scope::User).await?;
    /// for contact in contacts {
    ///     println!("联系人: {} (ID: {})", contact.name, contact.contact_id);
    /// }
    /// ```
    pub async fn search_contacts(
        &self,
        query: &str,
        limit: usize,
        scope: Scope,
        user_id: Option<&str>,
        include_archived: bool,
    ) -> Result<Vec<ContactDetail>, ShouxinError> {
        self.manager.search_contacts(query, limit, scope, user_id, include_archived).await
    }

    /// 联系人全息详情：人设 + 关联联系人 + 关联事件 + 相关记忆原文
    ///
    /// 与 `search_contacts` 的区别：后者只给联系人自身字段，
    /// 本方法额外聚合该联系人参与的事件与提到他的对话原文，
    /// 供智能体做"人物 + 关系 + 证据"的完整推理。
    pub async fn get_contact_detail(
        &self,
        contact_id: &str,
        scope: Scope,
        user_id: Option<&str>,
        event_limit: usize,
        memory_limit: usize,
    ) -> Result<serde_json::Value, ShouxinError> {
        self.manager
            .get_contact_detail(contact_id, scope, user_id, event_limit, memory_limit)
            .await
    }

    /// 搜索事件
    ///
    /// 搜索流程：
    /// 1. 查关系库事件索引表（快速检索）
    /// 2. 回源字典库获取完整内容
    ///
    /// # 参数
    /// - `filter`: 事件过滤器
    /// - `scope`: 操作范围，指定是操作用户数据还是智能体数据
    ///
    /// # 返回
    /// 事件详情列表
    ///
    /// # 示例
    /// ```ignore
    /// let filter = EventFilter::new()
    ///     .with_user_id("user_001".to_string())
    ///     .with_limit(20);
    ///
    /// let events = memory.search_events(filter, Scope::User).await?;
    /// for event in events {
    ///     println!("事件: {} (重要性: {})", event.description, event.importance);
    /// }
    /// ```
    pub async fn search_events(&self, filter: EventFilter, scope: Scope, user_id: Option<&str>) -> Result<Vec<EventDetail>, ShouxinError> {
        self.manager.search_events(filter, scope, user_id).await
    }

    /// 搜索历史对话
    ///
    /// 直接查字典库时序对话
    ///
    /// # 参数
    /// - `time_range`: 时间范围 (开始时间, 结束时间)
    /// - `scope`: 操作范围，指定是操作用户数据还是智能体数据
    ///
    /// # 返回
    /// 对话记录列表
    ///
    /// # 示例
    /// ```ignore
    /// let start = Utc::now() - chrono::Duration::days(7);
    /// let end = Utc::now();
    ///
    /// let conversations = memory.search_history((start, end), Scope::User).await?;
    /// for conv in conversations {
    ///     println!("时间: {} 内容: {}", conv.created_at, conv.content);
    /// }
    /// ```
    pub async fn search_history(
        &self,
        time_range: (DateTime<Utc>, DateTime<Utc>),
        scope: Scope,
        user_id: Option<&str>,
    ) -> Result<Vec<Conversation>, ShouxinError> {
        self.manager.search_history(time_range, scope, user_id).await
    }

    /// 语义搜索
    ///
    /// 调用向量库搜索
    ///
    /// # 参数
    /// - `query`: 查询文本
    /// - `limit`: 结果数量限制
    /// - `scope`: 操作范围，指定是操作用户数据还是智能体数据
    ///
    /// # 返回
    /// 学习素材搜索结果列表
    ///
    /// # 示例
    /// ```ignore
    /// let results = memory.semantic_search("Rust 编程", 10, Scope::User).await?;
    /// for result in results {
    ///     println!("内容: {} (相似度: {})");
    /// }
    /// ```
    pub async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        scope: Scope,
        user_id: Option<&str>,
        source_types: Option<Vec<String>>,
    ) -> Result<Vec<VectorSearchResult>, ShouxinError> {
        self.manager.semantic_search(query, limit, scope, user_id, source_types).await
    }

    /// 获取学习素材详情
    ///
    /// # 参数
    /// - `id`: 学习素材 ID
    ///
    /// # 返回
    /// 学习素材详情，如果不存在则返回 None
    ///
    /// # 示例
    /// ```ignore
    /// let material = memory.get_learning_material("1").await?;
    /// if let Some(material) = material {
    ///     println!("内容: {}");
    /// }
    /// ```
    pub async fn get_learning_material(
        &self,
        id: &str,
    ) -> Result<Option<LearningMaterial>, ShouxinError> {
        self.manager.get_learning_material(id).await
    }

    /// 添加待办事项
    ///
    /// # 参数
    /// - `todo`: 待办事项
    ///
    /// # 返回
    /// 操作结果
    ///
    /// # 示例
    /// ```ignore
    /// let todo = TodoItem::new(
    ///     "todo-123".to_string(),
    ///     vec!["contact-456".to_string()],
    ///     "买高铁票".to_string(),
    ///     Utc::now() + chrono::Duration::days(7),
    ///     "long_term".to_string(),
    /// );
    /// 添加待办事项
    ///
    /// # 参数
    /// - `todo`: 待办事项
    ///
    /// # 返回
    /// 操作结果
    ///
    /// # 示例
    /// ```ignore
    /// let todo = TodoItem::new(
    ///     "task-123".to_string(),
    ///     vec!["contact-123"].to_vec(),
    ///     "user-123".to_string(),
    ///     "personal".to_string(),
    ///     "完成项目".to_string(),
    ///     Some("详细描述".to_string()),
    ///     Some(Utc::now() + Duration::days(7)),
    ///     "pending".to_string(),
    ///     Utc::now(),
    ///     None,
    ///     "high".to_string(),
    ///     Some("event-123".to_string()),
    /// );
    /// memory.add_todo(&todo).await?;
    /// ```
    pub async fn add_todo(&self, todo: &TodoItem) -> Result<(), ShouxinError> {
        let todo_store = self.manager.get_todo_store();
        todo_store.add_todo(todo).await?;

        // 写时触发：待办添加后检查归档/提醒阈值
        let rel = self.manager.get_relational_store();
        let owner_type_str = match todo.owner_type {
            crate::core::contact::OwnerType::User => "User",
            crate::core::contact::OwnerType::Agent => "Agent",
            _ => "User",
        };
        let owner_id = &todo.owner_id;

        let pending_count = rel.count_pending_todos(owner_type_str, owner_id).await.unwrap_or(0);
        let policy = &self.todo_settings;

        // Archive: 超过 max_pending_before_archive 时归档最旧的一批
        if pending_count > policy.max_pending_before_archive as i64 {
            match rel.archive_stale_todos(
                owner_type_str,
                owner_id,
                policy.archive_keep_count,
                policy.archive_after_days,
            ).await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("[ArchiveOnWrite] Archived {} stale todos for {}/{}", count, owner_type_str, owner_id);
                    }
                }
                Err(e) => tracing::warn!("[ArchiveOnWrite] Failed to archive stale todos: {}", e),
            }
        }

        // Reminder: 使用归档后的最新数量，避免刚归档完又立刻提醒
        let pending_after_archive = rel.count_pending_todos(owner_type_str, owner_id).await.unwrap_or(0);
        if pending_after_archive > policy.reminder_threshold as i64 {
            if !rel.has_recent_reminder(owner_type_str, owner_id).await.unwrap_or(true) {
                let reminder_id = uuid::Uuid::new_v4().to_string();
                let title = format!("⚠️ 你有 {} 个待办未处理，请及时处理", pending_after_archive);
                if let Err(e) = rel.insert_reminder_todo(&reminder_id, owner_type_str, owner_id, &title).await {
                    tracing::warn!("[ArchiveOnWrite] Failed to insert reminder todo: {}", e);
                } else {
                    tracing::info!("[ArchiveOnWrite] Generated reminder todo for {}/{}", owner_type_str, owner_id);
                }
            }
        }

        Ok(())
    }

    pub async fn validate_contacts(
        &self,
        contact_ids: &[String],
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<(), String> {
        let relational_store = self.manager.get_relational_store();
        for contact_id in contact_ids {
            if contact_id.is_empty() {
                continue;
            }
            match relational_store
                .get_contact_by_id(contact_id, owner_type, owner_id)
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Err(format!("Contact not found: {}", contact_id));
                }
                Err(e) => {
                    return Err(format!("Contact lookup error: {}", e));
                }
            }
        }
        Ok(())
    }

    /// 获取待确认事件合并列表 (v0.5.1)
    pub async fn get_pending_event_merges(
        &self,
        owner_type: Option<crate::core::contact::OwnerType>,
        owner_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<crate::stores::relational::PendingEventMerge>, ShouxinError> {
        self.manager.get_pending_event_merges(owner_type, owner_id, limit).await
    }

    /// 确认事件合并 (v0.5.1)
    pub async fn confirm_event_merge(
        &self,
        merge_id: i64,
        action: &str,
        source_event_summary: Option<&str>,
    ) -> Result<String, ShouxinError> {
        self.manager.confirm_event_merge(merge_id, action, source_event_summary).await
    }

    fn get_todo_store(&self) -> crate::stores::relational::TodoListStore {
        self.manager.get_todo_store()
    }

    /// 获取待办事项列表
    ///
    /// # 参数
    /// - `filter`: 待办过滤器
    /// - `scope`: 操作范围，指定是操作用户数据还是智能体数据
    ///
    /// # 返回
    /// 待办事项列表
    ///
    /// # 示例
    /// ```ignore
    /// // 获取所有待处理的待办
    /// let filter = TodoFilter::new()
    ///     .with_status("pending".to_string())
    ///     .with_limit(20);
    ///
    /// let todos = memory.list_todos(filter, Scope::User).await?;
    /// for todo in todos {
    ///     println!("待办: {} (截止: {})", todo.title, todo.deadline);
    /// }
    /// ```
    pub async fn list_todos(&self, filter: TodoFilter, scope: Scope, user_id: Option<&str>) -> Result<Vec<TodoItem>, ShouxinError> {
        let (owner_type, owner_id) = scope.to_owner_params(user_id);
        let todo_store = self.get_todo_store();
        todo_store.get_all_todos(owner_type, &owner_id, filter.status.as_deref(), filter.limit, filter.include_archived).await
    }

    /// 完成待办事项
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    ///
    /// # 返回
    /// 操作结果
    ///
    /// # 示例
    /// ```ignore
    /// memory.complete_todo("todo-123").await?;
    /// ```
    pub async fn complete_todo(
        &self,
        todo_id: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<(), ShouxinError> {
        let todo_store = self.get_todo_store();
        todo_store.complete_todo(todo_id, owner_type, owner_id).await
    }

    /// 删除待办事项
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    ///
    /// # 返回
    /// 操作结果
    ///
    /// # 示例
    /// ```ignore
    /// memory.delete_todo("todo-123").await?;
    /// ```
    pub async fn delete_todo(
        &self,
        todo_id: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<(), ShouxinError> {
        let todo_store = self.get_todo_store();
        todo_store.delete_todo(todo_id, owner_type, owner_id).await
    }

    /// 获取待办事项详情
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    ///
    /// # 返回
    /// 待办事项详情，如果不存在则返回 None
    pub async fn get_todo(
        &self,
        todo_id: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<Option<TodoItem>, ShouxinError> {
        let todo_store = self.get_todo_store();
        todo_store.get_todo(todo_id, owner_type, owner_id).await
    }

    pub async fn get_todo_owner(
        &self,
        todo_id: &str,
    ) -> Result<Option<(crate::core::contact::OwnerType, String)>, ShouxinError> {
        let todo_store = self.get_todo_store();
        todo_store.get_todo_owner(todo_id).await
    }

    /// 获取待处理的待办事项
    ///
    /// # 参数
    /// - `limit`: 限制返回数量
    /// - `scope`: 操作范围，指定是操作用户数据还是智能体数据
    ///
    /// # 返回
    /// 待处理的待办事项列表（按截止时间升序）
    pub async fn list_pending_todos(&self, limit: i64, scope: Scope, user_id: Option<&str>) -> Result<Vec<TodoItem>, ShouxinError> {
        let (owner_type, owner_id) = scope.to_owner_params(user_id);
        let todo_store = self.get_todo_store();
        todo_store.get_pending_todos(owner_type, &owner_id, limit).await
    }

    /// 根据联系人获取待办事项
    ///
    /// # 参数
    /// - `contact_id`: 联系人唯一 ID
    /// - `scope`: 操作范围，指定是操作用户数据还是智能体数据
    ///
    /// # 返回
    /// 该联系人相关的待办事项列表
    pub async fn get_todos_by_contact(&self, contact_id: &str, scope: Scope, user_id: Option<&str>) -> Result<Vec<TodoItem>, ShouxinError> {
        let (owner_type, owner_id) = scope.to_owner_params(user_id);
        let todo_store = self.get_todo_store();
        todo_store.get_todos_by_contact(contact_id, owner_type, &owner_id).await
    }

    // ==================== 兼容旧 API 的方法 ====================
    // 以下方法提供与旧 MemoryManager API 的兼容性

    /// 获取所有记忆（兼容方法）
    ///
    /// 注意：新架构中记忆分布在三库，此方法返回空列表
    /// 建议使用 search_history 或 semantic_search 替代
    #[deprecated(note = "请使用 search_history 或 semantic_search 替代")]
    pub async fn get_all(
        &self,
        _filter: Option<&crate::core::MemoryFilter>,
    ) -> Result<Vec<Memory>, ShouxinError> {
        // 新架构中记忆分布在三库，不再有统一的 get_all
        Ok(Vec::new())
    }

    /// 获取指定记忆（兼容方法）
    ///
    /// 注意：新架构中需要指定记忆类型才能查询
    #[deprecated(note = "请使用具体的查询方法替代")]
    pub async fn get(&self, _memory_id: &str) -> Result<Option<Memory>, ShouxinError> {
        // 新架构中需要知道记忆类型才能查询
        Ok(None)
    }

    /// 更新记忆（兼容方法）
    ///
    /// 注意：新架构中记忆更新需要通过具体方法
    #[deprecated(note = "新架构暂不支持直接更新记忆")]
    pub async fn update(&self, _memory: Memory) -> Result<(), ShouxinError> {
        Err(ShouxinError::NotImplemented(
            "Memory update is not supported in three-library architecture. Use specific update methods.".to_string()
        ))
    }

    /// 删除记忆（兼容方法）
    ///
    /// 注意：新架构中字典库数据永久保存，不支持删除
    #[deprecated(note = "字典库数据永久保存，不支持删除")]
    pub async fn delete(&self, _memory_id: &str) -> Result<(), ShouxinError> {
        // 字典库数据永久保存，不支持删除
        Err(ShouxinError::NotImplemented(
            "Memory deletion is not supported in three-library architecture. Dictionary store is permanent.".to_string()
        ))
    }

    /// 清空所有记忆（兼容方法）
    ///
    /// 注意：新架构中字典库数据永久保存，不支持清空
    #[deprecated(note = "字典库数据永久保存，不支持清空")]
    pub async fn clear(&self) -> Result<(), ShouxinError> {
        Err(ShouxinError::NotImplemented(
            "Memory clear is not supported in three-library architecture. Dictionary store is permanent.".to_string()
        ))
    }
}
