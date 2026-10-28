#![allow(deprecated)]

//! 同步记忆管理 API
//!
//! 提供同步的记忆管理 API，内部使用 tokio 运行时
//!
//! # 架构说明
//! 本模块是 `AsyncMemory` 的同步封装，提供相同的 API 但使用阻塞调用

use chrono::{DateTime, Utc};
use tokio::runtime::Runtime;

use crate::api::{AsyncMemory, TodoFilter};
use crate::core::contact::Scope;
use crate::core::Memory;
use crate::error::ShouxinError;
use crate::memory::three_library_manager::AddResult;
use crate::stores::relational::TodoItem;
use crate::stores::vector::types::{LearningMaterial, VectorSearchResult};
use crate::traits::store_traits::{ContactDetail, Conversation, EventDetail, EventFilter};

/// 同步记忆管理 API
///
/// 基于三库架构的同步记忆管理接口
///
/// # 示例
/// ```ignore
/// let memory = SyncMemory::new(async_memory);
///
/// // 添加对话
/// let result = memory.add("我是张三，我喜欢编程", Some("user_001"))?;
///
/// // 搜索联系人
/// let contacts = memory.search_contacts("张三", 10)?;
/// ```
pub struct SyncMemory {
    /// 异步记忆管理
    async_memory: AsyncMemory,
    /// Tokio 运行时
    runtime: Runtime,
}

impl SyncMemory {
    /// 创建新的 SyncMemory 实例
    ///
    /// # 参数
    /// - `async_memory`: 异步记忆管理实例
    ///
    /// # 返回
    /// SyncMemory 实例
    pub fn new(async_memory: AsyncMemory) -> Self {
        let runtime = Runtime::new().expect("Failed to create tokio runtime");

        Self {
            async_memory,
            runtime,
        }
    }

    /// 添加对话（核心方法）
    ///
    /// # 参数
    /// - `conversation`: 对话文本内容
    /// - `user_id`: 可选的用户 ID
    ///
    /// # 返回
    /// 添加结果
    pub fn add(&self, conversation: &str, user_id: Option<&str>) -> Result<AddResult, ShouxinError> {
        self.runtime
            .block_on(self.async_memory.add(conversation, user_id, Scope::User))
    }

    /// 搜索联系人
    ///
    /// # 参数
    /// - `query`: 搜索查询
    /// - `limit`: 结果数量限制
    ///
    /// # 返回
    /// 联系人详情列表
    pub fn search_contacts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactDetail>, ShouxinError> {
        self.runtime
            .block_on(self.async_memory.search_contacts(query, limit, Scope::User, None, false))
    }

    /// 搜索事件
    ///
    /// # 参数
    /// - `filter`: 事件过滤器
    ///
    /// # 返回
    /// 事件详情列表
    pub fn search_events(&self, filter: EventFilter) -> Result<Vec<EventDetail>, ShouxinError> {
        self.runtime
            .block_on(self.async_memory.search_events(filter, Scope::User, None))
    }

    /// 搜索历史对话
    ///
    /// # 参数
    /// - `time_range`: 时间范围 (开始时间, 结束时间)
    ///
    /// # 返回
    /// 对话记录列表
    pub fn search_history(
        &self,
        time_range: (DateTime<Utc>, DateTime<Utc>),
    ) -> Result<Vec<Conversation>, ShouxinError> {
        self.runtime
            .block_on(self.async_memory.search_history(time_range, Scope::User, None))
    }

    /// 语义搜索
    ///
    /// # 参数
    /// - `query`: 查询文本
    /// - `limit`: 结果数量限制
    ///
    /// # 返回
    /// 学习素材搜索结果列表
    pub fn semantic_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<VectorSearchResult>, ShouxinError> {
        self.runtime
            .block_on(self.async_memory.semantic_search(query, limit, Scope::User, None, None))
    }

    /// 获取学习素材详情
    ///
    /// # 参数
    /// - `id`: 学习素材 ID
    ///
    /// # 返回
    /// 学习素材详情，如果不存在则返回 None
    pub fn get_learning_material(&self, id: &str) -> Result<Option<LearningMaterial>, ShouxinError> {
        self.runtime
            .block_on(self.async_memory.get_learning_material(id))
    }

    /// 添加待办事项
    ///
    /// # 参数
    /// - `todo`: 待办事项
    ///
    /// # 返回
    /// 操作结果
    pub fn add_todo(&self, todo: &TodoItem) -> Result<(), ShouxinError> {
        self.runtime.block_on(self.async_memory.add_todo(todo))
    }

    /// 获取待办事项列表
    ///
    /// # 参数
    /// - `filter`: 待办过滤器
    ///
    /// # 返回
    /// 待办事项列表
    pub fn list_todos(&self, filter: TodoFilter) -> Result<Vec<TodoItem>, ShouxinError> {
        self.runtime.block_on(self.async_memory.list_todos(filter, Scope::User, None))
    }

    /// 完成待办事项
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    ///
    /// # 返回
    /// 操作结果
    pub fn complete_todo(&self, todo_id: &str) -> Result<(), ShouxinError> {
        self.runtime
            .block_on(self.async_memory.complete_todo(
                todo_id,
                crate::core::contact::OwnerType::User,
                "default_user",
            ))
    }

    /// 删除待办事项
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    ///
    /// # 返回
    /// 操作结果
    pub fn delete_todo(&self, todo_id: &str) -> Result<(), ShouxinError> {
        self.runtime
            .block_on(self.async_memory.delete_todo(
                todo_id,
                crate::core::contact::OwnerType::User,
                "default_user",
            ))
    }

    /// 获取待办事项详情
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    ///
    /// # 返回
    /// 待办事项详情
    pub fn get_todo(&self, todo_id: &str) -> Result<Option<TodoItem>, ShouxinError> {
        self.runtime.block_on(self.async_memory.get_todo(
            todo_id,
            crate::core::contact::OwnerType::User,
            "default_user",
        ))
    }

    /// 获取待处理的待办事项
    ///
    /// # 参数
    /// - `limit`: 限制返回数量
    ///
    /// # 返回
    /// 待处理的待办事项列表
    pub fn list_pending_todos(&self, limit: i64) -> Result<Vec<TodoItem>, ShouxinError> {
        self.runtime
            .block_on(self.async_memory.list_pending_todos(limit, Scope::User, None))
    }

    /// 根据联系人获取待办事项
    ///
    /// # 参数
    /// - `contact_id`: 联系人唯一 ID
    ///
    /// # 返回
    /// 该联系人相关的待办事项列表
    pub fn get_todos_by_contact(&self, contact_id: &str) -> Result<Vec<TodoItem>, ShouxinError> {
        self.runtime
            .block_on(self.async_memory.get_todos_by_contact(contact_id, Scope::User, None))
    }

    // ==================== 兼容旧 API 的方法 ====================

    /// 获取所有记忆（兼容方法）
    #[deprecated(note = "请使用 search_history 或 semantic_search 替代")]
    pub fn get_all(
        &self,
        filter: Option<&crate::core::MemoryFilter>,
    ) -> Result<Vec<Memory>, ShouxinError> {
        self.runtime.block_on(self.async_memory.get_all(filter))
    }

    /// 获取指定记忆（兼容方法）
    #[deprecated(note = "请使用具体的查询方法替代")]
    pub fn get(&self, memory_id: &str) -> Result<Option<Memory>, ShouxinError> {
        self.runtime.block_on(self.async_memory.get(memory_id))
    }

    /// 更新记忆（兼容方法）
    #[deprecated(note = "新架构暂不支持直接更新记忆")]
    pub fn update(&self, memory: Memory) -> Result<(), ShouxinError> {
        self.runtime.block_on(self.async_memory.update(memory))
    }

    /// 删除记忆（兼容方法）
    #[deprecated(note = "字典库数据永久保存，不支持删除")]
    pub fn delete(&self, memory_id: &str) -> Result<(), ShouxinError> {
        self.runtime.block_on(self.async_memory.delete(memory_id))
    }

    /// 清空所有记忆（兼容方法）
    #[deprecated(note = "字典库数据永久保存，不支持清空")]
    pub fn clear(&self) -> Result<(), ShouxinError> {
        self.runtime.block_on(self.async_memory.clear())
    }
}