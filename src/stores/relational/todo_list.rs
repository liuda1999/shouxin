//! 待办事项表操作

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{query, Pool, Row, Sqlite};
use std::sync::Arc;

use crate::error::ShouxinError;

/// 待办事项结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// 待办唯一 ID
    pub todo_id: String,
    /// 关联联系人 ID 列表（JSON 数组）
    pub contact_ids: Vec<String>,
    /// 所有者 ID
    pub owner_id: String,
    /// 所有者类型
    pub owner_type: crate::core::contact::OwnerType,
    /// 待办标题
    pub title: String,
    /// 详细描述
    pub description: Option<String>,
    /// 截止时间
    pub deadline: DateTime<Utc>,
    /// 状态（pending / completed / auto_invalid）
    pub status: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 完成时间
    pub completed_at: Option<DateTime<Utc>>,
    /// 重要性（short_term / long_term / critical）
    pub importance: String,
    /// 关联的原始事件 ID（若有）
    pub source_event_id: Option<String>,
    /// 待办类型（normal / ambiguity / confirmation）
    pub todo_type: String,
    /// 关联实体类型（ambiguity 等）
    pub entity_type: Option<String>,
    /// 关联实体 ID
    pub entity_id: Option<String>,
    /// 是否已归档（热冷分离）
    pub is_archived: bool,
    /// 归档时间
    pub archived_at: Option<DateTime<Utc>>,
    /// 最后更新时间
    pub last_updated_at: DateTime<Utc>,
}

impl TodoItem {
    /// 创建新的待办事项
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    /// - `contact_ids`: 关联联系人 ID 列表
    /// - `owner_id`: 所有者 ID
    /// - `owner_type`: 所有者类型
    /// - `title`: 待办标题
    /// - `deadline`: 截止时间
    /// - `importance`: 重要性
    ///
    /// # 返回
    /// TodoItem 实例
    pub fn new(
        todo_id: String,
        contact_ids: Vec<String>,
        owner_id: String,
        owner_type: crate::core::contact::OwnerType,
        title: String,
        deadline: DateTime<Utc>,
        importance: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            todo_id,
            contact_ids,
            owner_id,
            owner_type,
            title,
            description: None,
            deadline,
            status: "pending".to_string(),
            created_at: now,
            completed_at: None,
            importance,
            source_event_id: None,
            todo_type: "normal".to_string(),
            entity_type: None,
            entity_id: None,
            is_archived: false,
            archived_at: None,
            last_updated_at: now,
        }
    }

    /// 设置详细描述
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// 设置关联的原始事件 ID
    pub fn with_source_event(mut self, event_id: String) -> Self {
        self.source_event_id = Some(event_id);
        self
    }

    /// 设置待办类型
    pub fn with_todo_type(mut self, todo_type: String) -> Self {
        self.todo_type = todo_type;
        self
    }

    /// 设置关联实体类型和 ID（如 ambiguity）
    pub fn with_entity(mut self, entity_type: String, entity_id: String) -> Self {
        self.entity_type = Some(entity_type);
        self.entity_id = Some(entity_id);
        self
    }

    /// 从数据库行解析
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, ShouxinError> {
        let contact_ids_json: String = row.get("contact_ids");
        let contact_ids: Vec<String> = serde_json::from_str(&contact_ids_json).map_err(|e| {
            ShouxinError::DeserializationError(format!("Failed to parse content: {}", e))
        })?;

        let owner_id: String = row.get("owner_id");
        let owner_type_str: String = row.get("owner_type");

        // 解析 owner_type
        let owner_type = match owner_type_str.as_str() {
            "User" => crate::core::contact::OwnerType::User,
            "Contact" => crate::core::contact::OwnerType::Contact,
            "Agent" => crate::core::contact::OwnerType::Agent,
            "Public" => crate::core::contact::OwnerType::Public,
            _ => crate::core::contact::OwnerType::User, // 默认值
        };

        let deadline_str: String = row.get("deadline");
        let deadline = DateTime::parse_from_rfc3339(&deadline_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                ShouxinError::DeserializationError(format!("Failed to parse deadline: {}", e))
            })?;

        let created_at_str: String = row.get("created_at");
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                ShouxinError::DeserializationError(format!("Failed to parse created_at: {}", e))
            })?;

        let completed_at_str: Option<String> = row.get("completed_at");
        let completed_at = completed_at_str
            .map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| {
                        ShouxinError::DeserializationError(format!(
                            "Failed to parse completed_at: {}",
                            e
                        ))
                    })
            })
            .transpose()?;

        let is_archived_int: i64 = row.try_get("is_archived").unwrap_or(0);
        let archived_at_str: Option<String> = row.try_get("archived_at").ok().flatten();
        let archived_at = archived_at_str
            .map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| {
                        ShouxinError::DeserializationError(format!(
                            "Failed to parse archived_at: {}",
                            e
                        ))
                    })
            })
            .transpose()?;

        let last_updated_at_str: Option<String> = row.try_get("last_updated_at").ok().flatten();
        let last_updated_at = match last_updated_at_str {
            Some(s) if !s.is_empty() => DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            _ => Utc::now(),
        };

        Ok(Self {
            todo_id: row.get("todo_id"),
            contact_ids,
            owner_id,
            owner_type,
            title: row.get("title"),
            description: row.get("description"),
            deadline,
            status: row.get("status"),
            created_at,
            completed_at,
            importance: row.get("importance"),
            source_event_id: row.get("source_event_id"),
            todo_type: row.get("todo_type"),
            entity_type: row.get("entity_type"),
            entity_id: row.get("entity_id"),
            is_archived: is_archived_int != 0,
            archived_at,
            last_updated_at,
        })
    }
}

/// 待办事项存储
#[derive(Clone)]
pub struct TodoListStore {
    pool: Arc<Pool<Sqlite>>,
}

impl TodoListStore {
    /// 创建新的待办事项存储
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool: Arc::new(pool) }
    }

    /// 添加待办事项
    ///
    /// # 参数
    /// - `todo`: 待办事项
    ///
    /// # 返回
    /// 操作结果
    pub async fn add_todo(&self, todo: &TodoItem) -> Result<(), ShouxinError> {
        let contact_ids_json = serde_json::to_string(&todo.contact_ids)?;

        let result = query(
            r#"
            INSERT OR IGNORE INTO todo_list 
            (todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, last_updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&todo.todo_id)
        .bind(&contact_ids_json)
        .bind(&todo.owner_id)
        .bind(format!("{:?}", todo.owner_type))
        .bind(&todo.title)
        .bind(&todo.description)
        .bind(todo.deadline.to_rfc3339())
        .bind(&todo.status)
        .bind(todo.created_at.to_rfc3339())
        .bind(todo.completed_at.map(|t| t.to_rfc3339()))
        .bind(&todo.importance)
        .bind(&todo.source_event_id)
        .bind(&todo.todo_type)
        .bind(&todo.entity_type)
        .bind(&todo.entity_id)
        .bind(if todo.is_archived { 1 } else { 0 })
        .bind(todo.last_updated_at.to_rfc3339())
        .execute(self.pool.as_ref())
        .await?;

        if result.rows_affected() == 0 {
            return Err(ShouxinError::ConflictError(format!(
                "待办已存在: {}",
                todo.title
            )));
        }

        Ok(())
    }

    /// 获取待处理的待办事项
    ///
    /// # 参数
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    /// - `limit`: 限制返回数量
    ///
    /// # 返回
    /// 待处理的待办事项列表（按截止时间升序）
    pub async fn get_pending_todos(
        &self, 
        owner_type: crate::core::contact::OwnerType, 
        owner_id: &str, 
        limit: i64
    ) -> Result<Vec<TodoItem>, ShouxinError> {
        let rows = query(
            r#"
            SELECT todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, archived_at, last_updated_at
            FROM todo_list
            WHERE status = 'pending' AND is_archived = 0 AND owner_type = ? AND owner_id = ?
            ORDER BY deadline ASC
            LIMIT ?
            "#,
        )
        .bind(format!("{:?}", owner_type))
        .bind(owner_id)
        .bind(limit)
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut todos = Vec::new();
        for row in rows {
            todos.push(TodoItem::from_row(&row)?);
        }

        Ok(todos)
    }

    /// 完成待办事项
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    ///
    /// # 返回
    /// 操作结果
    pub async fn complete_todo(&self, todo_id: &str, owner_type: crate::core::contact::OwnerType, owner_id: &str) -> Result<(), ShouxinError> {
        let now = Utc::now();

        let result = query(
            r#"
            UPDATE todo_list 
            SET status = 'completed',
                completed_at = ?,
                last_updated_at = ?
            WHERE todo_id = ? AND owner_type = ? AND owner_id = ?
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .bind(todo_id)
        .bind(format!("{:?}", owner_type))
        .bind(owner_id)
        .execute(self.pool.as_ref())
        .await?;

        if result.rows_affected() == 0 {
            return Err(ShouxinError::StorageError(
                "Todo not found".to_string(),
            ));
        }

        Ok(())
    }

    /// 根据联系人获取待办事项
    ///
    /// # 参数
    /// - `contact_id`: 联系人唯一 ID
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 该联系人相关的待办事项列表
    ///
    /// # 说明
    /// 使用 LIKE 查询 JSON 数组中的联系人 ID（兼顾兼容性和安全性）
    pub async fn get_todos_by_contact(
        &self, 
        contact_id: &str, 
        owner_type: crate::core::contact::OwnerType, 
        owner_id: &str
    ) -> Result<Vec<TodoItem>, ShouxinError> {
        // 使用更安全的 LIKE 模式匹配 JSON 数组元素
        // 匹配三种可能的位置：数组开头、数组中间、数组结尾
        let patterns = vec![
            format!("[\"{}\"", contact_id),
            format!(",\"{}\"", contact_id),
            format!("\"{}\"]", contact_id),
        ];

        let mut todos = Vec::new();

        // 对每个 pattern 分别查询，然后合并结果并去重
        for pattern in patterns {
            let rows = query(
                r#"
                SELECT todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, archived_at, last_updated_at
                FROM todo_list
                WHERE contact_ids LIKE ? AND owner_type = ? AND owner_id = ? AND is_archived = 0
                ORDER BY deadline ASC
                "#,
            )
            .bind(&pattern)
            .bind(format!("{:?}", owner_type))
            .bind(owner_id)
            .fetch_all(self.pool.as_ref())
            .await?;

            for row in rows {
                let todo = TodoItem::from_row(&row)?;
                // 只添加尚未在列表中的待办
                if !todos.iter().any(|t: &TodoItem| t.todo_id == todo.todo_id) {
                    todos.push(todo);
                }
            }
        }

        // 按截止时间重新排序
        todos.sort_by_key(|a| a.deadline);

        Ok(todos)
    }

    /// 获取所有待办事项
    ///
    /// # 参数
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    /// - `status`: 状态过滤（可选）
    /// - `limit`: 限制返回数量
    ///
    /// # 返回
    /// 待办事项列表
    pub async fn get_all_todos(
        &self,
        owner_type: crate::core::contact::OwnerType, 
        owner_id: &str,
        status: Option<&str>,
        limit: i64,
        include_archived: bool,
    ) -> Result<Vec<TodoItem>, ShouxinError> {
        const SQL_STATUS_ARCHIVED: &str = "SELECT todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, archived_at, last_updated_at FROM todo_list WHERE status = ? AND owner_type = ? AND owner_id = ? AND is_archived = 0 ORDER BY deadline ASC LIMIT ?";
        const SQL_STATUS_ALL: &str = "SELECT todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, archived_at, last_updated_at FROM todo_list WHERE status = ? AND owner_type = ? AND owner_id = ? ORDER BY deadline ASC LIMIT ?";
        const SQL_ALL_ARCHIVED: &str = "SELECT todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, archived_at, last_updated_at FROM todo_list WHERE owner_type = ? AND owner_id = ? AND is_archived = 0 ORDER BY deadline ASC LIMIT ?";
        const SQL_ALL_ALL: &str = "SELECT todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, archived_at, last_updated_at FROM todo_list WHERE owner_type = ? AND owner_id = ? ORDER BY deadline ASC LIMIT ?";

        let rows = match (status, include_archived) {
            (Some(s), false) => {
                sqlx::query(SQL_STATUS_ARCHIVED)
                    .bind(s)
                    .bind(format!("{:?}", owner_type))
                    .bind(owner_id)
                    .bind(limit)
                    .fetch_all(self.pool.as_ref())
                    .await?
            }
            (Some(s), true) => {
                sqlx::query(SQL_STATUS_ALL)
                    .bind(s)
                    .bind(format!("{:?}", owner_type))
                    .bind(owner_id)
                    .bind(limit)
                    .fetch_all(self.pool.as_ref())
                    .await?
            }
            (None, false) => {
                sqlx::query(SQL_ALL_ARCHIVED)
                    .bind(format!("{:?}", owner_type))
                    .bind(owner_id)
                    .bind(limit)
                    .fetch_all(self.pool.as_ref())
                    .await?
            }
            (None, true) => {
                sqlx::query(SQL_ALL_ALL)
                    .bind(format!("{:?}", owner_type))
                    .bind(owner_id)
                    .bind(limit)
                    .fetch_all(self.pool.as_ref())
                    .await?
            }
        };

        let mut todos = Vec::new();
        for row in rows {
            todos.push(TodoItem::from_row(&row)?);
        }

        Ok(todos)
    }

    /// 删除待办事项
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    ///
    /// # 返回
    /// 操作结果
    pub async fn delete_todo(&self, todo_id: &str, owner_type: crate::core::contact::OwnerType, owner_id: &str) -> Result<(), ShouxinError> {
        let result = query(
            "DELETE FROM todo_list WHERE todo_id = ? AND owner_type = ? AND owner_id = ?"
        )
            .bind(todo_id)
            .bind(format!("{:?}", owner_type))
            .bind(owner_id)
            .execute(self.pool.as_ref())
            .await?;

        if result.rows_affected() == 0 {
            return Err(ShouxinError::StorageError(
                "Todo not found".to_string(),
            ));
        }

        Ok(())
    }

    /// 获取待办事项详情
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    ///
    /// # 返回
    /// 待办事项详情，如果不存在则返回 None
    pub async fn get_todo(&self, todo_id: &str, owner_type: crate::core::contact::OwnerType, owner_id: &str) -> Result<Option<TodoItem>, ShouxinError> {
        let row: Option<sqlx::sqlite::SqliteRow> = query(
            r#"
            SELECT todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, archived_at, last_updated_at
            FROM todo_list
            WHERE todo_id = ? AND owner_type = ? AND owner_id = ?
            "#,
        )
        .bind(todo_id)
        .bind(format!("{:?}", owner_type))
        .bind(owner_id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.and_then(|r| TodoItem::from_row(&r).ok()))
    }

    pub async fn get_todo_owner(&self, todo_id: &str) -> Result<Option<(crate::core::contact::OwnerType, String)>, ShouxinError> {
        let row: Option<sqlx::sqlite::SqliteRow> = query(
            "SELECT owner_type, owner_id FROM todo_list WHERE todo_id = ?"
        )
        .bind(todo_id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(row.map(|r| {
            let ot: String = r.get(0);
            let oi: String = r.get(1);
            let owner_type = match ot.as_str() {
                "Agent" => crate::core::contact::OwnerType::Agent,
                _ => crate::core::contact::OwnerType::User,
            };
            (owner_type, oi)
        }))
    }

    /// 模糊搜索待办标题
    ///
    /// # 参数
    /// - `title_hint`: 标题关键词（用于 LIKE 匹配）
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 匹配的待处理待办事项列表
    pub async fn search_pending_todos_by_title_fuzzy(
        &self,
        title_hint: &str,
        owner_type: &str,
        owner_id: &str,
    ) -> Result<Vec<TodoItem>, ShouxinError> {
        // 先做连续子串匹配（SQL LIKE，命中即最精确）
        let pattern = format!("%{}%", title_hint);
        let rows = query(
            r#"
            SELECT todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id
            FROM todo_list
            WHERE status = 'pending' AND owner_type = ?1 AND owner_id = ?2 AND title LIKE ?3
            "#,
        )
        .bind(owner_type)
        .bind(owner_id)
        .bind(&pattern)
        .fetch_all(self.pool.as_ref())
        .await?;

        let mut todos = Vec::new();
        for row in rows {
            todos.push(TodoItem::from_row(&row)?);
        }

        // 再补"模糊"匹配：关键词字符按顺序出现即可（允许中间夹其它字）
        // 例如 "写报告" 命中 "写项目报告" / "写周报报告"。
        // 旧实现只有 LIKE 连续子串，这类跨字匹配会漏掉，导致"更新待办"找不到目标。
        if !title_hint.trim().is_empty() {
            let hint: Vec<char> = title_hint.trim().chars().collect();
            if hint.len() >= 2 {
                let candidate_rows = query(
                    r#"
                    SELECT todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id
                    FROM todo_list
                    WHERE status = 'pending' AND owner_type = ?1 AND owner_id = ?2
                    LIMIT 500
                    "#,
                )
                .bind(owner_type)
                .bind(owner_id)
                .fetch_all(self.pool.as_ref())
                .await?;

                let mut seen: Vec<String> = todos.iter().map(|t| t.todo_id.clone()).collect();
                for row in candidate_rows {
                    let todo = TodoItem::from_row(&row)?;
                    if seen.contains(&todo.todo_id) {
                        continue;
                    }
                    if fuzzy_subsequence_match(&todo.title, &hint) {
                        seen.push(todo.todo_id.clone());
                        todos.push(todo);
                    }
                }
            }
        }

        Ok(todos)
    }

    /// 更新待办截止时间
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    /// - `new_deadline`: 新的截止时间（ISO 8601 格式）
    ///
    /// # 返回
    /// 操作结果
    pub async fn update_todo_deadline(
        &self,
        todo_id: &str,
        new_deadline: &str,
    ) -> Result<(), ShouxinError> {
        // F-T4: 必须同时刷新 last_updated_at，否则刚改过截止时间的待办
        // 仍会被"30 天未更新"的归档规则误判为陈旧。
        let now = Utc::now().to_rfc3339();
        query(
            "UPDATE todo_list SET deadline = ?1, last_updated_at = ?2 WHERE todo_id = ?3 AND status = 'pending'",
        )
        .bind(new_deadline)
        .bind(&now)
        .bind(todo_id)
        .execute(self.pool.as_ref())
        .await?;
        Ok(())
    }

    /// 清理旧待办（归档已完成的，永不删除）
    pub async fn cleanup(&self) -> Result<usize, ShouxinError> {
        let now = Utc::now().to_rfc3339();
        // 归档已完成的待办，而非删除
        let result = query(
            r#"
            UPDATE todo_list
            SET is_archived = 1, archived_at = ?
            WHERE status IN ('completed', 'auto_invalid') AND is_archived = 0
            "#
        )
        .bind(&now)
        .execute(self.pool.as_ref())
        .await?;

        Ok(result.rows_affected() as usize)
    }

    /// 将 deadline 已过期的 pending 待办标记为 auto_invalid
    pub async fn invalidate_expired_todos(&self) -> Result<usize, ShouxinError> {
        let now = Utc::now().to_rfc3339();
        let result = query(
            r#"
            UPDATE todo_list
            SET status = 'auto_invalid', last_updated_at = ?
            WHERE status = 'pending' AND is_archived = 0 AND deadline < ?
            "#,
        )
        .bind(&now)
        .bind(&now)
        .execute(self.pool.as_ref())
        .await?;

        if result.rows_affected() > 0 {
            tracing::info!(
                "[Todo Lifecycle] Invalidated {} expired todos (deadline passed)",
                result.rows_affected()
            );
        }

        Ok(result.rows_affected() as usize)
    }

    /// Count pending todos for a user
    pub async fn count_pending_todos(&self, owner_type: &str, owner_id: &str) -> Result<i64, ShouxinError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM todo_list WHERE status = 'pending' AND is_archived = 0 AND owner_type = ? AND owner_id = ?"
        )
        .bind(owner_type)
        .bind(owner_id)
        .fetch_one(self.pool.as_ref())
        .await?;
        Ok(row.0)
    }

    /// Count pending todos globally (all users)
    pub async fn count_all_pending_todos(&self) -> Result<i64, ShouxinError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM todo_list WHERE status = 'pending' AND is_archived = 0"
        )
        .fetch_one(self.pool.as_ref())
        .await?;
        Ok(row.0)
    }

    /// Get distinct owner_type/owner_id pairs with pending todos
    pub async fn get_all_todo_owners(&self) -> Result<Vec<(String, String)>, ShouxinError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT owner_type, owner_id FROM todo_list WHERE status = 'pending' AND is_archived = 0"
        )
        .fetch_all(self.pool.as_ref())
        .await?;
        Ok(rows)
    }

    /// Check if there's a recent reminder todo for today
    pub async fn has_recent_reminder(&self, owner_type: &str, owner_id: &str) -> Result<bool, ShouxinError> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM todo_list WHERE owner_type = ? AND owner_id = ? AND todo_type = 'reminder' AND created_at >= ?"
        )
        .bind(owner_type)
        .bind(owner_id)
        .bind(&today)
        .fetch_one(self.pool.as_ref())
        .await?;
        Ok(row.0 > 0)
    }

    /// 归档陈旧待办（超过 `after_days` 天未更新），并最多保留 `keep_count` 条 pending
    ///
    /// 阈值来自 `[todo]` 配置，不再写死 30/50。
    pub async fn archive_stale_todos(&self, owner_type: &str, owner_id: &str, keep_count: usize, after_days: i64) -> Result<usize, ShouxinError> {
        let count = self.count_pending_todos(owner_type, owner_id).await?;
        if (count as usize) <= keep_count {
            return Ok(0);
        }
        let to_archive = (count as usize) - keep_count;
        let now = chrono::Utc::now().to_rfc3339();
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(after_days.max(0))).to_rfc3339();

        let result = sqlx::query(
            "UPDATE todo_list SET is_archived = 1, archived_at = ? WHERE todo_id IN (SELECT todo_id FROM todo_list WHERE status = 'pending' AND is_archived = 0 AND owner_type = ? AND owner_id = ? AND last_updated_at < ? ORDER BY last_updated_at ASC LIMIT ?)"
        )
        .bind(&now)
        .bind(owner_type)
        .bind(owner_id)
        .bind(&cutoff)
        .bind(to_archive as i64)
        .execute(self.pool.as_ref())
        .await?;

        Ok(result.rows_affected() as usize)
    }

    /// Insert a reminder todo
    pub async fn insert_reminder_todo(
        &self,
        todo_id: &str,
        owner_type: &str,
        owner_id: &str,
        title: &str,
    ) -> Result<(), ShouxinError> {
        let now = chrono::Utc::now();
        // P1-6: 使用 to_rfc3339() 保持 deadline 格式一致（+00:00 而非 Z），避免唯一索引格式不匹配
        let end_of_day = now.date_naive().and_hms_opt(23, 59, 59)
            .map(|t| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(t, chrono::Utc))
            .unwrap_or(now)
            .to_rfc3339();
        let now_str = now.to_rfc3339();
        // P1-6: 使用 INSERT OR IGNORE 防止 TOCTOU 竞态导致唯一约束冲突
        let result = sqlx::query(
            "INSERT OR IGNORE INTO todo_list (todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, importance, todo_type, is_archived, last_updated_at) VALUES (?, '[]', ?, ?, ?, NULL, ?, 'pending', ?, 'low', 'reminder', 0, ?)"
        )
        .bind(todo_id)
        .bind(owner_id)
        .bind(owner_type)
        .bind(title)
        .bind(&end_of_day)
        .bind(&now_str)
        .bind(&now_str)
        .execute(self.pool.as_ref())
        .await?;

        if result.rows_affected() == 0 {
            tracing::info!("[P1-6] insert_reminder_todo: reminder already exists, ignored (title='{}')", title);
        }
        Ok(())
    }
}

/// 模糊子序列匹配：关键词字符按顺序出现在标题中即可（中间可夹其它字符）
///
/// 例："写报告" 可匹配 "写项目报告"、"写周报报告"。
fn fuzzy_subsequence_match(title: &str, hint: &[char]) -> bool {
    let title_chars: Vec<char> = title.chars().collect();
    let mut idx = 0usize;
    for hc in hint {
        let mut found = false;
        while idx < title_chars.len() {
            let tc = title_chars[idx];
            idx += 1;
            if tc == *hc || tc.to_lowercase().eq(hc.to_lowercase()) {
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod fuzzy_match_tests {
    use super::fuzzy_subsequence_match;

    #[test]
    fn test_subsequence_match() {
        let hint: Vec<char> = "写报告".chars().collect();
        assert!(fuzzy_subsequence_match("写项目报告", &hint));
        assert!(fuzzy_subsequence_match("写周报报告", &hint));
        assert!(fuzzy_subsequence_match("写报告", &hint));
        assert!(!fuzzy_subsequence_match("写代码", &hint));
        assert!(!fuzzy_subsequence_match("报告写", &hint));
    }
}
