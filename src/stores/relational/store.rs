//! 关系库实现（SQLite 存储）
//!
//! 提供轻量快速索引功能

use chrono::{DateTime, Utc};
use sqlx::{query, Pool, Row, Sqlite};
use tracing::debug;

use super::todo_list::{TodoItem, TodoListStore};
use super::virtual_entity::VirtualEntityStore;
use crate::core::{ContactIndex, EventIndex};
use crate::error::ShouxinError;
use async_trait::async_trait;
use crate::traits::store_traits::{RelationalStoreTrait, ContactIndex as TraitContactIndex, EventIndex as TraitEventIndex, EventFilter};

use super::file_store::FileStore;

/// 关系库存储实现
///
/// 使用 SQLite 作为存储引擎，提供快速索引
pub struct RelationalStore {
    /// SQLite 连接池
    pool: Pool<Sqlite>,
    /// 待办事项存储
    todo_list_store: TodoListStore,
    /// 文件存储
    file_store: FileStore,
    /// 虚拟角色别名存储
    virtual_alias_store: VirtualEntityStore,
}

impl RelationalStore {
    /// 创建新的关系库实例
    ///
    /// # 参数
    /// - `pool`: SQLite 连接池
    ///
    /// # 返回
    /// RelationalStore 实例
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self {
            todo_list_store: TodoListStore::new(pool.clone()),
            file_store: FileStore::new(pool.clone()),
            virtual_alias_store: VirtualEntityStore::new(pool.clone()),
            pool,
        }
    }

    /// 获取文件存储的引用
    pub fn file_store(&self) -> &FileStore {
        &self.file_store
    }

    /// 获取虚拟角色别名存储的引用
    pub fn virtual_alias_store(&self) -> &VirtualEntityStore {
        &self.virtual_alias_store
    }

    /// 检查名称是否匹配虚拟角色
    ///
    /// # 参数
    /// - `name`: 待检查的名称
    ///
    /// # 返回
    /// 如果匹配则返回匹配的虚拟角色信息，否则返回 None
    pub async fn check_virtual_character_match(&self, name: &str) -> Result<Option<super::virtual_entity::VirtualCharacterInfo>, ShouxinError> {
        self.virtual_alias_store.check_virtual_match(name).await
    }

    /// 获取数据库连接池的引用
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    /// 初始化数据库表结构
    ///
    /// 在首次启动时执行，创建必要的表和索引
    pub async fn init_schema(&self) -> Result<(), ShouxinError> {
        // 创建联系人索引表
        query(
            r#"
            CREATE TABLE IF NOT EXISTS contact_index (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                aliases TEXT,
                is_self INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                last_interaction_at TEXT NOT NULL,
                related_contact_ids TEXT,
                relationship_tags TEXT,
                relation_weight TEXT,
                background TEXT,
                notes TEXT,
                owner_type TEXT NOT NULL DEFAULT 'User',
                owner_id TEXT NOT NULL DEFAULT 'default_user',
                is_agent INTEGER DEFAULT 0,
                parent_agent_id TEXT,
                is_archived INTEGER DEFAULT 0,
                archived_at TEXT,
                activity_score REAL DEFAULT 0.0
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // P1-5: 为已有数据库迁移 activity_score 列
        query("ALTER TABLE contact_index ADD COLUMN activity_score REAL DEFAULT 0.0")
            .execute(&self.pool)
            .await
            .ok(); // 忽略"列已存在"错误

        // 创建事件索引表
        query(
            r#"
            CREATE TABLE IF NOT EXISTS event_index (
                event_id TEXT PRIMARY KEY,
                contact_ids TEXT,
                owner_id TEXT NOT NULL DEFAULT '__self__',
                owner_type TEXT NOT NULL DEFAULT 'User',
                time_range_start TEXT NOT NULL,
                time_range_end TEXT NOT NULL,
                importance TEXT,
                status TEXT,
                created_at TEXT NOT NULL,
                completed_at TEXT,
                summary TEXT,
                event_type TEXT,
                deadline TEXT,
                auto_invalid_at TEXT,
                is_auto_invalid INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        let _ = query(
            "ALTER TABLE event_index ADD COLUMN title TEXT"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "ALTER TABLE event_index ADD COLUMN is_merged INTEGER DEFAULT 0"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "ALTER TABLE event_index ADD COLUMN merge_target_id TEXT"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "ALTER TABLE event_index ADD COLUMN last_merge_check_at TEXT"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "ALTER TABLE event_index ADD COLUMN event_time TEXT"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "ALTER TABLE event_index ADD COLUMN importance_score REAL DEFAULT 0.5"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "ALTER TABLE event_index ADD COLUMN is_archived INTEGER DEFAULT 0"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "ALTER TABLE event_index ADD COLUMN archived_at TEXT"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            r#"
            CREATE TABLE IF NOT EXISTS pending_event_merge (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_event_id TEXT NOT NULL,
                target_event_id TEXT,
                source_event_summary TEXT NOT NULL,
                target_event_summary TEXT,
                similarity_score REAL NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                resolved_at TEXT,
                resolution_action TEXT,
                owner_type TEXT NOT NULL,
                owner_id TEXT NOT NULL
            )
            "#
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "CREATE INDEX IF NOT EXISTS idx_pending_merge_status ON pending_event_merge(status, created_at DESC)"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "CREATE INDEX IF NOT EXISTS idx_pending_merge_source ON pending_event_merge(source_event_id)"
        )
        .execute(&self.pool)
        .await;

        let _ = query(
            "CREATE INDEX IF NOT EXISTS idx_pending_merge_owner ON pending_event_merge(owner_type, owner_id)"
        )
        .execute(&self.pool)
        .await;

        // 创建待办事项表
        query(
            r#"
            CREATE TABLE IF NOT EXISTS todo_list (
                todo_id TEXT PRIMARY KEY,
                contact_ids TEXT NOT NULL,
                owner_id TEXT NOT NULL DEFAULT '__self__',
                owner_type TEXT NOT NULL DEFAULT 'User',
                title TEXT NOT NULL,
                description TEXT,
                deadline TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                completed_at TEXT,
                importance TEXT NOT NULL,
                source_event_id TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // ========== Schema 迁移（v0.3.0 多所有者隔离）==========
        // 为已有的 contact_index 表补充 owner_type/owner_id/is_agent/parent_agent_id 列
        // 使用 try-catch 模式：如果列已存在则忽略错误
        let migration_queries = [
            "ALTER TABLE contact_index ADD COLUMN owner_type TEXT NOT NULL DEFAULT 'User'",
            "ALTER TABLE contact_index ADD COLUMN owner_id TEXT NOT NULL DEFAULT 'default_user'",
            "ALTER TABLE contact_index ADD COLUMN is_agent INTEGER DEFAULT 0",
            "ALTER TABLE contact_index ADD COLUMN parent_agent_id TEXT",
        ];
        for mq in &migration_queries {
            let _ = query(mq).execute(&self.pool).await;
        }

        // 创建索引
        query("CREATE INDEX IF NOT EXISTS idx_contact_name ON contact_index(name)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_contact_last_interaction ON contact_index(last_interaction_at DESC)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_contact_is_self ON contact_index(is_self)")
            .execute(&self.pool)
            .await?;

        // V9: 同一 owner 下相同名称的联系人唯一约束，防止并发去重竞态
        let _ = query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_contact_name_owner ON contact_index(name, owner_id, owner_type)",
        )
        .execute(&self.pool)
        .await;

        query(
            "CREATE INDEX IF NOT EXISTS idx_event_time_start ON event_index(time_range_start DESC)",
        )
        .execute(&self.pool)
        .await?;

        query("CREATE INDEX IF NOT EXISTS idx_event_time_end ON event_index(time_range_end DESC)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_event_status ON event_index(status)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_event_owner ON event_index(owner_id, owner_type)")
            .execute(&self.pool)
            .await?;

        // V1: 同一 owner 下相同摘要的事件唯一约束，防止去重竞态
        let _ = query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_event_summary ON event_index(summary, owner_id, owner_type)",
        )
        .execute(&self.pool)
        .await;

        // 为待办事项表创建索引
        query("CREATE INDEX IF NOT EXISTS idx_todo_status ON todo_list(status)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_todo_deadline ON todo_list(deadline ASC)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_todo_importance ON todo_list(importance)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_todo_owner ON todo_list(owner_id, owner_type)")
            .execute(&self.pool)
            .await?;

        // D3/B2: 同一 owner 下相同标题+deadline 的 pending 待办唯一约束
        // 先删除旧索引（如果存在），再创建包含 deadline 的新索引
        // P1-6: 不再静默吞没错误，记录日志以便诊断索引创建失败
        if let Err(e) = query("DROP INDEX IF EXISTS idx_unique_pending_todo")
            .execute(&self.pool)
            .await
        {
            tracing::warn!("[P1-6] Failed to drop old idx_unique_pending_todo: {}", e);
        }
        match query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_pending_todo ON todo_list(title, owner_id, owner_type, deadline) WHERE status = 'pending'",
        )
        .execute(&self.pool)
        .await
        {
            Ok(_) => tracing::info!("[P1-6] Unique index idx_unique_pending_todo created successfully"),
            Err(e) => tracing::error!(
                "[P1-6] Failed to create idx_unique_pending_todo (deduplication will NOT work): {}",
                e
            ),
        }

        // 创建实体别名表（用于实体归一化）
        query(
            r#"
            CREATE TABLE IF NOT EXISTS entity_alias (
                canonical_id TEXT NOT NULL,
                alias TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (canonical_id, alias)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 创建切片关联关系表
        query(
            r#"
            CREATE TABLE IF NOT EXISTS slice_relation (
                source_slice_id TEXT NOT NULL,
                target_slice_id TEXT NOT NULL,
                relation_type TEXT NOT NULL,
                weight REAL DEFAULT 1.0,
                created_at TEXT NOT NULL,
                PRIMARY KEY (source_slice_id, target_slice_id, relation_type)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 创建切片关键词表（倒排索引）
        query(
            r#"
            CREATE TABLE IF NOT EXISTS slice_keyword (
                slice_id TEXT NOT NULL,
                keyword TEXT NOT NULL,
                weight REAL DEFAULT 1.0,
                created_at TEXT NOT NULL,
                PRIMARY KEY (slice_id, keyword)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 为新表创建索引
        query("CREATE INDEX IF NOT EXISTS idx_entity_alias_alias ON entity_alias(alias)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_slice_relation_source ON slice_relation(source_slice_id)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_slice_relation_target ON slice_relation(target_slice_id)")
            .execute(&self.pool)
            .await?;

        query("CREATE INDEX IF NOT EXISTS idx_slice_keyword_keyword ON slice_keyword(keyword)")
            .execute(&self.pool)
            .await?;

        self.init_schema_v0_5_0().await?;

        // v0.5.1: todo_list 热冷分离字段
        let _ = query(
            "ALTER TABLE todo_list ADD COLUMN is_archived INTEGER DEFAULT 0"
        ).execute(&self.pool).await;
        let _ = query(
            "ALTER TABLE todo_list ADD COLUMN archived_at TEXT"
        ).execute(&self.pool).await;
        let _ = query(
            "ALTER TABLE todo_list ADD COLUMN last_updated_at TEXT DEFAULT CURRENT_TIMESTAMP"
        ).execute(&self.pool).await;

        self.file_store.init_schema().await?;

        // 虚拟角色独立表（virtual_character / virtual_character_alias / event_character_link）
        self.virtual_alias_store.init_schema().await?;

        Ok(())
    }

    async fn init_schema_v0_5_0(&self) -> Result<(), ShouxinError> {
        let _ = query(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version TEXT PRIMARY KEY,
                executed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )"
        ).execute(&self.pool).await;

        let already_executed: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM schema_migrations WHERE version = 'v0.5.1'"
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(false);

        if already_executed {
            return Ok(());
        }

        let alter_queries = [
            "ALTER TABLE contact_index ADD COLUMN is_virtual INTEGER DEFAULT 0",
            "ALTER TABLE event_index ADD COLUMN canonical_event_id TEXT DEFAULT NULL",
            "ALTER TABLE event_index ADD COLUMN merged_from_ids TEXT DEFAULT NULL",
            "ALTER TABLE event_index ADD COLUMN ambiguity_flag INTEGER DEFAULT 0",
            "ALTER TABLE todo_list ADD COLUMN todo_type TEXT NOT NULL DEFAULT 'normal'",
            "ALTER TABLE todo_list ADD COLUMN entity_type TEXT DEFAULT NULL",
            "ALTER TABLE todo_list ADD COLUMN entity_id TEXT DEFAULT NULL",
            "ALTER TABLE todo_list ADD COLUMN resolved_answer TEXT DEFAULT NULL",
            "ALTER TABLE todo_list ADD COLUMN resolved_at TEXT DEFAULT NULL",
            "ALTER TABLE contact_index ADD COLUMN is_archived INTEGER DEFAULT 0",
            "ALTER TABLE contact_index ADD COLUMN archived_at TEXT",
        ];
        for mq in &alter_queries {
            let _ = query(mq).execute(&self.pool).await;
        }

        let create_queries = [
            r#"CREATE TABLE IF NOT EXISTS memory_processing_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_processed_ts TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z',
                last_profile_update_ts TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z',
                heartbeat_count INTEGER NOT NULL DEFAULT 0,
                consecutive_skips INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )"#,
            r#"INSERT OR IGNORE INTO memory_processing_state (id, last_processed_ts, last_profile_update_ts)
               VALUES (1, '1970-01-01T00:00:00Z', '1970-01-01T00:00:00Z')"#,
            r#"CREATE TABLE IF NOT EXISTS contact_profile_update_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                contact_id TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                update_count_today INTEGER NOT NULL DEFAULT 1,
                UNIQUE(contact_id, updated_at)
            )"#,
            r#"CREATE TABLE IF NOT EXISTS contact_merge_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                primary_contact_id TEXT NOT NULL,
                merged_contact_id TEXT NOT NULL,
                merge_type TEXT NOT NULL DEFAULT 'auto',
                confidence_score REAL NOT NULL,
                user_confirmed INTEGER DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                confirmed_at TEXT DEFAULT NULL
            )"#,
            r#"CREATE TABLE IF NOT EXISTS event_merge_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                primary_event_id TEXT NOT NULL,
                merged_event_id TEXT NOT NULL,
                merge_reason TEXT NOT NULL,
                time_diff_hours REAL,
                similarity_score REAL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )"#,
            r#"CREATE TABLE IF NOT EXISTS ambiguity_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ambiguity_type TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                description TEXT NOT NULL,
                context_summary TEXT,
                resolved_todo_id TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                resolved_at TEXT DEFAULT NULL
            )"#,
            r#"CREATE TABLE IF NOT EXISTS contact_alias_index (
                alias TEXT NOT NULL,
                contact_id TEXT NOT NULL,
                owner_type TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                is_primary INTEGER DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (alias, contact_id, owner_type, owner_id)
            )"#,
            r#"CREATE TABLE IF NOT EXISTS virtual_character_aliases (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                contact_id TEXT NOT NULL,
                alias_name TEXT NOT NULL,
                source_work TEXT,
                alias_type TEXT DEFAULT 'nickname',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (contact_id) REFERENCES contact_index(id) ON DELETE CASCADE
            )"#,
        ];
        for cq in &create_queries {
            let _ = query(cq).execute(&self.pool).await;
        }

        let index_queries = [
            "CREATE INDEX IF NOT EXISTS idx_event_canonical ON event_index(canonical_event_id) WHERE canonical_event_id IS NOT NULL",
            "CREATE INDEX IF NOT EXISTS idx_event_ambiguity ON event_index(ambiguity_flag) WHERE ambiguity_flag = 1",
            "CREATE INDEX IF NOT EXISTS idx_todo_type ON todo_list(todo_type) WHERE todo_type = 'ambiguity'",
            "CREATE INDEX IF NOT EXISTS idx_todo_entity ON todo_list(entity_type, entity_id) WHERE entity_type IS NOT NULL",
            "CREATE INDEX IF NOT EXISTS idx_todo_resolved ON todo_list(resolved_at) WHERE resolved_at IS NOT NULL",
            "CREATE INDEX IF NOT EXISTS idx_profile_update_contact ON contact_profile_update_log(contact_id, updated_at)",
            "CREATE INDEX IF NOT EXISTS idx_merge_primary ON contact_merge_log(primary_contact_id)",
            "CREATE INDEX IF NOT EXISTS idx_merge_merged ON contact_merge_log(merged_contact_id)",
            "CREATE INDEX IF NOT EXISTS idx_event_merge_primary ON event_merge_log(primary_event_id)",
            "CREATE INDEX IF NOT EXISTS idx_ambiguity_status ON ambiguity_log(status, created_at DESC)",
            "CREATE INDEX IF NOT EXISTS idx_ambiguity_entity ON ambiguity_log(entity_type, entity_id)",
            "CREATE INDEX IF NOT EXISTS idx_alias_lookup ON contact_alias_index(alias, owner_type, owner_id)",
            "CREATE INDEX IF NOT EXISTS idx_contact_aliases ON contact_alias_index(contact_id)",
            "CREATE INDEX IF NOT EXISTS idx_vca_contact ON virtual_character_aliases(contact_id)",
            "CREATE INDEX IF NOT EXISTS idx_vca_alias ON virtual_character_aliases(alias_name)",
        ];
        for iq in &index_queries {
            let _ = query(iq).execute(&self.pool).await;
        }

        let _ = query(
            "INSERT INTO schema_migrations (version) VALUES ('v0.5.1')"
        ).execute(&self.pool).await;

        Ok(())
    }

    /// 插入或更新联系人索引
    ///
    /// # 参数
    /// - `contact`: 联系人索引记录
    ///
    /// # 返回
    /// 操作结果
    pub async fn upsert_contact_index(&self, contact: &ContactIndex) -> Result<(), ShouxinError> {
        query(
            r#"
            INSERT INTO contact_index
            (id, name, aliases, is_self, created_at, last_interaction_at, related_contact_ids,
             relationship_tags, relation_weight, background, notes, owner_type, owner_id,
             is_agent, parent_agent_id, is_virtual, is_archived, activity_score)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?)
            ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            aliases = excluded.aliases,
            is_self = excluded.is_self,
            last_interaction_at = excluded.last_interaction_at,
            related_contact_ids = excluded.related_contact_ids,
            relationship_tags = excluded.relationship_tags,
            relation_weight = excluded.relation_weight,
            background = excluded.background,
            notes = excluded.notes,
            owner_type = excluded.owner_type,
            owner_id = excluded.owner_id,
            is_agent = excluded.is_agent,
            parent_agent_id = excluded.parent_agent_id,
            is_virtual = excluded.is_virtual
            -- P1-5: 不覆盖 activity_score，保留累积值（由 update_contact_activity 单独维护）
            "#,
        )
        .bind(&contact.id)
        .bind(&contact.name)
        .bind(&contact.aliases)
        .bind(if contact.is_self { 1 } else { 0 })
        .bind(contact.created_at.to_rfc3339())
        .bind(contact.last_interaction_at.to_rfc3339())
        .bind(&contact.related_contact_ids)
        .bind(&contact.relationship_tags)
        .bind(&contact.relation_weight)
        .bind(&contact.background)
        .bind(&contact.notes)
        .bind(format!("{:?}", contact.owner_type))
        .bind(&contact.owner_id)
        .bind(if contact.is_agent { 1 } else { 0 })
        .bind(&contact.parent_agent_id)
        .bind(if contact.is_virtual { 1 } else { 0 })
        .bind(contact.activity_score)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 获取活跃联系人列表
    ///
    /// # 参数
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    /// - `limit`: 返回数量限制
    ///
    /// # 返回
    /// 联系人索引列表，按最后交互时间降序排列
    pub async fn get_active_contacts(
        &self, 
        owner_type: crate::core::contact::OwnerType, 
        owner_id: &str, 
        limit: i64
    ) -> Result<Vec<ContactIndex>, ShouxinError> {
        // 先获取用户记录（is_self=1），然后获取其他联系人，确保用户记录始终在列表中
        let user_rows = query(
            r#"
            SELECT id, name, aliases, is_self, created_at, last_interaction_at,
                   related_contact_ids, relationship_tags, relation_weight, background, notes,
                   owner_type, owner_id, is_agent, parent_agent_id, is_virtual, is_archived, archived_at, activity_score
            FROM contact_index
            WHERE is_self = 1 AND owner_type = ? AND owner_id = ? AND is_archived = 0
            "#,
        )
        .bind(format!("{:?}", owner_type))
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await?;

        // 获取其他联系人，按最后交互时间降序
        let other_rows = query(
            r#"
            SELECT id, name, aliases, is_self, created_at, last_interaction_at,
                   related_contact_ids, relationship_tags, relation_weight, background, notes,
                   owner_type, owner_id, is_agent, parent_agent_id, is_virtual, is_archived, archived_at, activity_score
            FROM contact_index
            WHERE is_self = 0 AND owner_type = ? AND owner_id = ? AND is_archived = 0
            ORDER BY last_interaction_at DESC
            LIMIT ?
            "#,
        )
        .bind(format!("{:?}", owner_type))
        .bind(owner_id)
        .bind(limit - user_rows.len() as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut contacts = Vec::new();

        // 先添加用户记录
        for row in user_rows {
            contacts.push(ContactIndex::from_row(&row)?);
        }

        // 再添加其他联系人
        for row in other_rows {
            contacts.push(ContactIndex::from_row(&row)?);
        }

        Ok(contacts)
    }

    /// 按名称精确搜索联系人
    ///
    /// # 参数
    /// - `name`: 联系人姓名
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 匹配的联系人列表（按最后交互时间降序）
    pub async fn search_contacts_by_name(
        &self,
        name: &str,
        owner_type: &str,
        owner_id: &str,
    ) -> Result<Vec<ContactIndex>, ShouxinError> {
        let rows = sqlx::query(
            "SELECT id, name, aliases, is_self, created_at, last_interaction_at, \
             related_contact_ids, relationship_tags, relation_weight, background, notes, \
             owner_type, owner_id, is_agent, parent_agent_id, is_virtual, is_archived, archived_at, activity_score \
             FROM contact_index WHERE name = ? AND owner_type = ? AND owner_id = ? AND is_archived = 0 \
             ORDER BY last_interaction_at DESC"
        )
        .bind(name)
        .bind(owner_type)
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await?;

        let mut contacts = Vec::new();
        for row in rows {
            contacts.push(ContactIndex::from_row(&row).map_err(|e| {
                tracing::error!("[search_contacts_by_name] from_row failed for name='{}': {:?}", name, e);
                ShouxinError::DatabaseError(format!("decode contact_index row failed: {}", e))
            })?);
        }
        Ok(contacts)
    }

    /// 按摘要精确搜索事件
    ///
    /// # 参数
    /// - `summary`: 事件摘要
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 匹配的事件列表
    pub async fn search_events_by_summary(
        &self,
        summary: &str,
        owner_type: &str,
        owner_id: &str,
    ) -> Result<Vec<EventIndex>, ShouxinError> {
        let rows = sqlx::query(
            "SELECT event_id, contact_ids, owner_id, owner_type, time_range_start, time_range_end, \
             importance, status, created_at, completed_at, summary, title, event_type, deadline, \
             auto_invalid_at, is_auto_invalid, is_merged, merge_target_id, last_merge_check_at, event_time, \
             importance_score, is_archived, archived_at \
             FROM event_index WHERE summary = ? AND owner_type = ? AND owner_id = ?"
        )
        .bind(summary)
        .bind(owner_type)
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();
        for row in rows {
            events.push(EventIndex::from_row(&row)?);
        }
        Ok(events)
    }

    /// 删除联系人索引
    ///
    /// # 参数
    /// - `id`: 联系人唯一标识
    ///
    /// # 返回
    /// 操作结果
    pub async fn delete_contact_index(&self, id: &str) -> Result<(), ShouxinError> {
        // 禁止删除用户记录
        if id == "__self__" {
            return Ok(());
        }

        query("DELETE FROM contact_index WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// 插入事件索引
    ///
    /// # 参数
    /// - `event`: 事件索引记录
    ///
    /// # 返回
    /// 操作结果
    pub async fn insert_event_index(&self, event: &EventIndex) -> Result<(), ShouxinError> {
        let result = query(
            r#"
            INSERT OR IGNORE INTO event_index 
            (event_id, contact_ids, owner_id, owner_type, time_range_start, time_range_end, importance, 
             status, created_at, completed_at, summary, title, event_type, deadline, auto_invalid_at, is_auto_invalid,
             is_merged, merge_target_id, last_merge_check_at, event_time, importance_score, is_archived, archived_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&event.event_id)
        .bind(&event.contact_ids)
        .bind(&event.owner_id)
        .bind(format!("{:?}", event.owner_type))
        .bind(event.time_range_start.to_rfc3339())
        .bind(event.time_range_end.to_rfc3339())
        .bind(&event.importance)
        .bind(&event.status)
        .bind(event.created_at.to_rfc3339())
        .bind(event.completed_at.map(|t| t.to_rfc3339()))
        .bind(&event.summary)
        .bind(&event.title)
        .bind(&event.event_type)
        .bind(event.deadline.map(|t| t.to_rfc3339()))
        .bind(event.auto_invalid_at.map(|t| t.to_rfc3339()))
        .bind(if event.is_auto_invalid { 1 } else { 0 })
        .bind(if event.is_merged { 1 } else { 0 })
        .bind(&event.merge_target_id)
        .bind(event.last_merge_check_at.map(|t| t.to_rfc3339()))
        .bind(event.event_time.map(|t| t.to_rfc3339()))
        .bind(event.importance_score)
        .bind(if event.is_archived { 1 } else { 0 })
        .bind(event.archived_at.map(|t| t.to_rfc3339()))
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            tracing::debug!("事件已存在（数据库去重拦截）: {}", event.summary);
        }

        Ok(())
    }

    /// 根据联系人 ID 搜索事件
    ///
    /// # 参数
    /// - `contact_id`: 联系人唯一标识
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 事件索引列表
    pub async fn search_events_by_contact(
        &self,
        contact_id: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<Vec<EventIndex>, ShouxinError> {
        // 使用更宽松的模式匹配，同时匹配带引号和不带引号的情况
        // 并考虑可能的空格
        let patterns = vec![
            format!("%\"{}\"%", contact_id),
            format!("%{}%", contact_id),
        ];

        let mut events = Vec::new();

        // 对每个 pattern 分别查询，然后合并结果并去重
        for pattern in patterns {
            let rows = query(
                r#"
                SELECT event_id, contact_ids, owner_id, owner_type, time_range_start, time_range_end,
                       importance, status, created_at, completed_at, summary, title, event_type, deadline,
                       auto_invalid_at, is_auto_invalid, is_merged, merge_target_id, last_merge_check_at,
                       event_time, importance_score, is_archived, archived_at
                FROM event_index
                WHERE contact_ids LIKE ? AND owner_type = ? AND owner_id = ?
                ORDER BY time_range_start DESC
                "#,
            )
            .bind(&pattern)
            .bind(format!("{:?}", owner_type))
            .bind(owner_id)
            .fetch_all(&self.pool)
            .await?;

            for row in rows {
                let event = EventIndex::from_row(&row)?;
                // 只添加尚未在列表中的事件
                if !events
                    .iter()
                    .any(|e: &EventIndex| e.event_id == event.event_id)
                {
                    events.push(event);
                }
            }
        }

        // 按时间范围开始重新排序
        events.sort_by_key(|b| std::cmp::Reverse(b.time_range_start));

        Ok(events)
    }

    /// 搜索联系人（支持 owner 过滤）
    ///
    /// 在 contact_index 表中按名称模糊搜索联系人，同时过滤所有者。
    ///
    /// # 参数
    /// - `search_query`: 搜索关键词
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    /// - `limit`: 返回结果数量限制
    /// 联系人检索：**热表优先，不够再查冷表**
    ///
    /// 设计约定（与产品一致）：
    /// - 热表（`is_archived = 0`）只保留常用/近期联系人（外部按 50 条上限裁剪），平时查询都命中热表，快；
    /// - 热表里没查够，再补查冷表（`is_archived = 1`），避免"人被归档了就搜不到"；
    /// - 虚拟角色（`is_virtual = 1`）默认**不混进普通联系人结果**，它们走 `/api/virtual_entities`。
    pub async fn search_contacts_with_owner(
        &self,
        search_query: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
        limit: usize,
        include_archived: bool,
        include_virtual: bool,
    ) -> Result<Vec<TraitContactIndex>, ShouxinError> {
        let mut collected: Vec<TraitContactIndex> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        // 第一段：热表
        let hot = self
            .query_contacts_page(search_query, owner_type, owner_id, limit, false, include_virtual)
            .await?;
        for c in hot {
            if seen.insert(c.contact_id.clone()) {
                collected.push(c);
            }
        }

        // 第二段：热表没查够（或没命中）时补冷表
        if collected.len() < limit {
            let remain = limit - collected.len();
            let cold = self
                .query_contacts_page(search_query, owner_type, owner_id, remain, true, include_virtual)
                .await?;
            for c in cold {
                if seen.insert(c.contact_id.clone()) {
                    collected.push(c);
                }
            }
        }

        // include_archived=true 时，调用方要的是"全量"，这里补上热表里漏掉的归档项
        if include_archived {
            let all = self
                .query_contacts_page(search_query, owner_type, owner_id, limit, true, include_virtual)
                .await?;
            for c in all {
                if seen.insert(c.contact_id.clone()) {
                    collected.push(c);
                }
            }
            collected.truncate(limit);
        }

        Ok(collected)
    }

    /// 单段查询（热表/冷表各查一次）
    async fn query_contacts_page(
        &self,
        search_query: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
        limit: usize,
        archived: bool,
        include_virtual: bool,
    ) -> Result<Vec<TraitContactIndex>, ShouxinError> {
        let like_pattern = format!("%{}%", search_query);
        let owner_type_str = format!("{:?}", owner_type);
        let virtual_filter = if include_virtual { "" } else { " AND c.is_virtual = 0" };

        let sql = format!(
            r#"
            SELECT DISTINCT c.id, c.name, c.created_at, c.last_interaction_at
            FROM contact_index c
            LEFT JOIN contact_alias_index a ON c.id = a.contact_id AND c.owner_type = a.owner_type AND c.owner_id = a.owner_id
            WHERE (c.name LIKE ? OR a.alias LIKE ?)
              AND c.owner_type = ? AND c.owner_id = ?
              AND c.is_archived = ?{}
            ORDER BY c.last_interaction_at DESC
            LIMIT ?
            "#,
            virtual_filter
        );

        let rows = sqlx::query(&sql)
            .bind(&like_pattern)
            .bind(&like_pattern)
            .bind(&owner_type_str)
            .bind(owner_id)
            .bind(if archived { 1 } else { 0 })
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;

        let mut contacts = Vec::new();
        for row in rows {
            let id: String = row.get("id");
            let name: String = row.get("name");
            let created: String = row.get("created_at");
            let last: String = row.get("last_interaction_at");
            contacts.push(TraitContactIndex {
                contact_id: id,
                name,
                user_id: None,
                activity_score: 0.0,
                last_activity: chrono::DateTime::parse_from_rfc3339(&last)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                rank: 0,
                created_at: chrono::DateTime::parse_from_rfc3339(&created)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            });
        }
        Ok(contacts)
    }

    /// 搜索事件（支持 owner 过滤）
    ///
    /// 在 event_index 表中按条件搜索事件，同时过滤所有者。
    ///
    /// # 参数
    /// - `filter`: 事件过滤器
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    pub async fn search_events_with_owner(
        &self,
        filter: &EventFilter,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<Vec<TraitEventIndex>, ShouxinError> {
        let mut sql = String::from(
            "SELECT event_id, time_range_start, created_at, importance, summary, event_type, title
             FROM event_index WHERE owner_type = ? AND owner_id = ?"
        );

        let mut params: Vec<String> = vec![
            format!("{:?}", owner_type),
            owner_id.to_string(),
        ];

        // 热冷分离：默认只返回未归档事件（与 trait search_events 保持一致）
        if !filter.include_archived {
            sql.push_str(" AND is_archived = 0");
        }

        if let Some(ref importance) = filter.importance {
            sql.push_str(" AND importance = ?");
            params.push(importance.clone());
        }

        if let Some(ref status) = filter.status {
            sql.push_str(" AND status = ?");
            params.push(status.clone());
        }

        if let Some(start) = filter.start_time {
            sql.push_str(" AND time_range_start >= ?");
            params.push(start.to_rfc3339());
        }

        if let Some(end) = filter.end_time {
            sql.push_str(" AND time_range_start <= ?");
            params.push(end.to_rfc3339());
        }

        if let Some(ref query) = filter.search_query {
            sql.push_str(" AND (summary LIKE ? OR event_id LIKE ?)");
            params.push(format!("%{}%", query));
            params.push(format!("%{}%", query));
        }

        sql.push_str(" ORDER BY time_range_start DESC LIMIT ?");
        params.push(filter.limit.to_string());

        let mut q = sqlx::query(&sql);
        for param in &params {
            q = q.bind(param);
        }

        let rows = q.fetch_all(&self.pool).await?;

        let mut events = Vec::new();
        for row in rows {
            let event_id: String = row.get("event_id");
            let time_range_start_str: String = row.get("time_range_start");
            let created_at_str: String = row.get("created_at");
            let importance_val: Option<String> = row.get("importance");
            let summary: Option<String> = row.get("summary");
            let event_type_val: Option<String> = row.get("event_type");
            let title_val: Option<String> = row.try_get("title").ok().flatten();

            let event_time = DateTime::parse_from_rfc3339(&time_range_start_str)
                .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                .with_timezone(&Utc);
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                .with_timezone(&Utc);

            let importance = match importance_val.as_deref() {
                Some("high") => 1.0_f32,
                Some("medium") => 0.6,
                Some("low") => 0.3,
                Some("short_term") => 0.5,
                Some("long_term") => 0.8,
                Some("critical") => 1.0,
                _ => 0.3,
            };

            let event = TraitEventIndex {
                event_id,
                title: title_val,
                event_type: event_type_val.unwrap_or_else(|| summary.clone().unwrap_or_default()),
                user_id: None,
                agent_id: None,
                event_time,
                archive_id: String::new(),
                memory_id: None,
                importance,
                created_at,
            };
            events.push(event);
        }

        Ok(events)
    }

    /// 更新事件状态
    ///
    /// # 参数
    /// - `event_id`: 事件唯一标识
    /// - `status`: 新状态
    /// - `completed_at`: 完成时间
    ///
    /// # 返回
    /// 操作结果
    pub async fn update_event_status(
        &self,
        event_id: &str,
        status: &str,
        completed_at: DateTime<Utc>,
    ) -> Result<(), ShouxinError> {
        query(
            r#"
            UPDATE event_index
            SET status = ?, completed_at = ?
            WHERE event_id = ?
            "#,
        )
        .bind(status)
        .bind(completed_at.to_rfc3339())
        .bind(event_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 标记事件为已完成
    ///
    /// # 参数
    /// - `event_id`: 事件唯一标识
    ///
    /// # 返回
    /// 操作结果
    ///
    /// # 说明
    /// 将事件状态更新为 'completed'，并记录完成时间
    pub async fn mark_event_completed(&self, event_id: &str) -> Result<(), ShouxinError> {
        let completed_at = Utc::now();
        self.update_event_status(event_id, "completed", completed_at)
            .await
    }

    /// 更新联系人背景信息
    ///
    /// # 参数
    /// - `contact_id`: 联系人唯一标识
    /// - `background`: 背景信息 JSON 字符串
    ///
    /// # 返回
    /// 操作结果
    pub async fn update_contact_background(
        &self,
        contact_id: &str,
        background: &str,
    ) -> Result<(), ShouxinError> {
        query(
            r#"
            UPDATE contact_index
            SET background = ?
            WHERE id = ?
            "#,
        )
        .bind(background)
        .bind(contact_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 更新联系人之间的关系（关联联系人 ID 列表 + 关系标签）
    ///
    /// 这两列此前从未被写入：画像生成时虽然从 LLM 拿到了 `social_relations`，
    /// 但只塞进了 `background` 的 JSON 里，关系列一直是 `[]`，
    /// 于是"联系人之间的关系"在检索侧永远为空。
    pub async fn update_contact_relations(
        &self,
        contact_id: &str,
        related_contact_ids_json: &str,
        relationship_tags_json: &str,
    ) -> Result<(), ShouxinError> {
        query(
            r#"
            UPDATE contact_index
            SET related_contact_ids = ?, relationship_tags = ?
            WHERE id = ?
            "#,
        )
        .bind(related_contact_ids_json)
        .bind(relationship_tags_json)
        .bind(contact_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 批量取联系人的画像字段
    ///
    /// `search_contacts_with_owner` 只查了 id/name/时间等基础列，
    /// 别名、画像、关系字段需要单独补齐（否则接口只能返回空值）。
    pub async fn fetch_contact_profiles(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, ContactProfileFields>, ShouxinError> {
        let mut map = std::collections::HashMap::new();
        if ids.is_empty() {
            return Ok(map);
        }

        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, aliases, background, relationship_tags, related_contact_ids, is_virtual \
             FROM contact_index WHERE id IN ({})",
            placeholders
        );

        let mut q = sqlx::query(&sql);
        for id in ids {
            q = q.bind(id);
        }
        let rows = q.fetch_all(&self.pool).await?;

        for row in rows {
            let id: String = row.get("id");
            map.insert(
                id,
                ContactProfileFields {
                    aliases: row.get::<Option<String>, _>("aliases"),
                    background: row.get::<Option<String>, _>("background"),
                    relationship_tags: row.get::<Option<String>, _>("relationship_tags"),
                    related_contact_ids: row.get::<Option<String>, _>("related_contact_ids"),
                    is_virtual: row.try_get::<i64, _>("is_virtual").unwrap_or(0) != 0,
                },
            );
        }

        // 别名的第二个来源：contact_alias_index（抽取链路写入的昵称/简称）
        //
        // 两份数据历史上是分开的：抽取器把 "明哥" 这类别名写进 alias 表，
        // 心跳把"遇到的名字"写进 contact_index.aliases。
        // 展示时应取并集，否则别名看起来总是只有名字本身。
        let alias_sql = format!(
            "SELECT contact_id, alias FROM contact_alias_index WHERE contact_id IN ({})",
            placeholders
        );
        let mut aq = sqlx::query(&alias_sql);
        for id in ids {
            aq = aq.bind(id);
        }
        let mut extra: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for row in aq.fetch_all(&self.pool).await.unwrap_or_default() {
            let cid: String = row.get("contact_id");
            let alias: String = row.get("alias");
            extra.entry(cid).or_default().push(alias);
        }

        for (cid, aliases) in extra {
            if let Some(fields) = map.get_mut(&cid) {
                let mut merged: Vec<String> = fields
                    .aliases
                    .as_ref()
                    .and_then(|raw| serde_json::from_str(raw).ok())
                    .unwrap_or_default();
                for a in aliases {
                    if !merged.contains(&a) {
                        merged.push(a);
                    }
                }
                fields.aliases = Some(
                    serde_json::to_string(&merged).unwrap_or_else(|_| "[]".to_string()),
                );
            }
        }

        Ok(map)
    }

    /// 按 ID 批量取联系人姓名（用于把关联联系人 ID 解析成名字）
    pub async fn fetch_contact_names(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, String>, ShouxinError> {
        let mut map = std::collections::HashMap::new();
        if ids.is_empty() {
            return Ok(map);
        }

        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT id, name FROM contact_index WHERE id IN ({})", placeholders);
        let mut q = sqlx::query(&sql);
        for id in ids {
            q = q.bind(id);
        }
        for row in q.fetch_all(&self.pool).await? {
            let id: String = row.get("id");
            let name: String = row.get("name");
            map.insert(id, name);
        }

        Ok(map)
    }

    /// 根据联系人 ID 获取联系人信息
    ///
    /// # 参数
    /// - `contact_id`: 联系人唯一标识
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 联系人索引信息
    pub async fn get_contact_by_id(
        &self,
        contact_id: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<Option<ContactIndex>, ShouxinError> {
        let row = query(
            r#"
            SELECT id, name, aliases, is_self, created_at, last_interaction_at,
                   related_contact_ids, relationship_tags, relation_weight, background, notes,
                   owner_type, owner_id, is_agent, parent_agent_id, is_virtual, is_archived, archived_at, activity_score
            FROM contact_index
            WHERE id = ? AND owner_type = ? AND owner_id = ?
            "#,
        )
        .bind(contact_id)
        .bind(format!("{:?}", owner_type))
        .bind(owner_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(ContactIndex::from_row(&r)?)),
            None => Ok(None),
        }
    }

    /// 添加待办事项
    ///
    /// # 参数
    /// - `todo`: 待办事项
    ///
    /// # 返回
    /// 操作结果
    pub async fn add_todo(&self, todo: &TodoItem) -> Result<(), ShouxinError> {
        self.todo_list_store.add_todo(todo).await
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
        self.todo_list_store.get_pending_todos(owner_type, owner_id, limit).await
    }

    /// 完成待办事项
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 操作结果
    pub async fn complete_todo(
        &self,
        todo_id: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<(), ShouxinError> {
        self.todo_list_store.complete_todo(todo_id, owner_type, owner_id).await
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
    pub async fn get_todos_by_contact(
        &self, 
        contact_id: &str, 
        owner_type: crate::core::contact::OwnerType, 
        owner_id: &str
    ) -> Result<Vec<TodoItem>, ShouxinError> {
        self.todo_list_store.get_todos_by_contact(contact_id, owner_type, owner_id).await
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
        self.todo_list_store.get_all_todos(owner_type, owner_id, status, limit, include_archived).await
    }

    /// 删除待办事项
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 操作结果
    pub async fn delete_todo(
        &self,
        todo_id: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<(), ShouxinError> {
        self.todo_list_store.delete_todo(todo_id, owner_type, owner_id).await
    }

    /// 获取待办事项详情
    ///
    /// # 参数
    /// - `todo_id`: 待办唯一 ID
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 待办事项详情，如果不存在则返回 None
    pub async fn get_todo(
        &self,
        todo_id: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<Option<TodoItem>, ShouxinError> {
        self.todo_list_store.get_todo(todo_id, owner_type, owner_id).await
    }

    pub async fn get_todo_owner(
        &self,
        todo_id: &str,
    ) -> Result<Option<(crate::core::contact::OwnerType, String)>, ShouxinError> {
        self.todo_list_store.get_todo_owner(todo_id).await
    }
}

/// ContactIndex 的数据库行解析实现
impl ContactIndex {
    /// 从数据库行解析 ContactIndex
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        let created_at_str: String = row.get("created_at");
        let last_interaction_at_str: String = row.get("last_interaction_at");
        let is_self: i64 = row.get("is_self");
        let owner_type_str: String = row.get("owner_type");
        let owner_id: String = row.get("owner_id");
        let is_agent_int: i64 = row.get("is_agent");
        let parent_agent_id: Option<String> = row.get("parent_agent_id");
        let is_virtual_int: i64 = row.try_get("is_virtual").unwrap_or(0);
        let is_archived_int: i64 = row.try_get("is_archived").unwrap_or(0);
        let archived_at_str: Option<String> = row.try_get("archived_at").ok().flatten();

        // 解析 owner_type
        let owner_type = match owner_type_str.as_str() {
            "User" => crate::core::contact::OwnerType::User,
            "Contact" => crate::core::contact::OwnerType::Contact,
            "Agent" => crate::core::contact::OwnerType::Agent,
            "Public" => crate::core::contact::OwnerType::Public,
            _ => crate::core::contact::OwnerType::User, // 默认值
        };

        Ok(Self {
            id: row.get("id"),
            name: row.get("name"),
            aliases: row.get("aliases"),
            is_self: is_self == 1,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_interaction_at: DateTime::parse_from_rfc3339(&last_interaction_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            related_contact_ids: row.get("related_contact_ids"),
            relationship_tags: row.get("relationship_tags"),
            relation_weight: row.get("relation_weight"),
            background: row.get("background"),
            notes: row.get("notes"),
            owner_type,
            owner_id,
            is_agent: is_agent_int != 0,
            parent_agent_id,
            is_virtual: is_virtual_int != 0,
            is_archived: is_archived_int != 0,
            archived_at: archived_at_str
                .filter(|s| !s.is_empty())
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            activity_score: row.try_get("activity_score").unwrap_or(0.0),
        })
    }
}

/// EventIndex 的数据库行解析实现
impl EventIndex {
    /// 从数据库行解析 EventIndex
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        let time_range_start_str: String = row.get("time_range_start");
        let time_range_end_str: String = row.get("time_range_end");
        let created_at_str: String = row.get("created_at");
        let completed_at_str: Option<String> = row.get("completed_at");
        let event_type_str: Option<String> = row.get("event_type");
        let deadline_str: Option<String> = row.get("deadline");
        let auto_invalid_at_str: Option<String> = row.get("auto_invalid_at");
        let is_auto_invalid_int: Option<i64> = row.get("is_auto_invalid");
        let is_merged_int: Option<i64> = row.get("is_merged");
        let merge_target_id: Option<String> = row.get("merge_target_id");
        let last_merge_check_at_str: Option<String> = row.get("last_merge_check_at");
        let event_time_str: Option<String> = row.get("event_time");
        let importance_score: f32 = row.try_get::<f32, _>("importance_score").unwrap_or(0.5);
        let is_archived_int: Option<i64> = row.get("is_archived");
        let archived_at_str: Option<String> = row.get("archived_at");
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

        Ok(Self {
            event_id: row.get("event_id"),
            contact_ids: row.get("contact_ids"),
            owner_id,
            owner_type,
            time_range_start: DateTime::parse_from_rfc3339(&time_range_start_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            time_range_end: DateTime::parse_from_rfc3339(&time_range_end_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            importance: row.get("importance"),
            status: row.get("status"),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            completed_at: completed_at_str
                .filter(|s| !s.is_empty())
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            summary: row.get("summary"),
            title: row.try_get("title").ok().flatten(),
            event_type: event_type_str.unwrap_or_default(),
            deadline: deadline_str
                .filter(|s| !s.is_empty())
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            auto_invalid_at: auto_invalid_at_str
                .filter(|s| !s.is_empty())
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            is_auto_invalid: is_auto_invalid_int.unwrap_or(0) != 0,
            is_merged: is_merged_int.unwrap_or(0) != 0,
            merge_target_id,
            last_merge_check_at: last_merge_check_at_str
                .filter(|s| !s.is_empty())
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            event_time: event_time_str
                .filter(|s| !s.is_empty())
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            importance_score,
            is_archived: is_archived_int.unwrap_or(0) != 0,
            archived_at: archived_at_str
                .filter(|s| !s.is_empty())
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
        })
    }
}

impl RelationalStore {
    /// 插入联系人别名索引
    pub async fn insert_contact_alias(
        &self,
        contact_id: &str,
        alias: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<(), ShouxinError> {
        let owner_type_str = format!("{:?}", owner_type);
        query(
            r#"
            INSERT OR IGNORE INTO contact_alias_index 
            (alias, contact_id, owner_type, owner_id, is_primary, created_at)
            VALUES (?, ?, ?, ?, 0, datetime('now'))
            "#,
        )
        .bind(alias)
        .bind(contact_id)
        .bind(&owner_type_str)
        .bind(owner_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 插入联系人别名（批量）
    pub async fn insert_contact_aliases(
        &self,
        contact_id: &str,
        aliases: &[String],
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<(), ShouxinError> {
        for alias in aliases {
            self.insert_contact_alias(contact_id, alias, owner_type, owner_id).await?;
        }
        Ok(())
    }

    /// 打开持久化存储的关系库
    ///
    /// # 参数
    /// - `path`: 数据库文件路径
    ///
    /// # 返回
    /// RelationalStore 实例
    pub async fn open(path: &str) -> Result<Self, ShouxinError> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(20)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&format!("sqlite:{}?mode=rwc", path))
            .await?;

        let store = Self::new(pool);
        store.init_schema().await?;
        Ok(store)
    }

    /// 创建内存模式的关系库（用于测试）
    pub async fn in_memory() -> Result<Self, ShouxinError> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(20)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect("sqlite::memory:")
            .await?;

        let store = Self::new(pool);
        store.init_schema().await?;
        Ok(store)
    }

    /// 获取待办事项存储
    pub fn get_todo_store(&self) -> TodoListStore {
        self.todo_list_store.clone()
    }

    /// 清理待办事项
    pub async fn cleanup_todos(&self) -> Result<usize, ShouxinError> {
        self.todo_list_store.cleanup().await
    }

    /// 统计用户的待办数
    pub async fn count_pending_todos(&self, owner_type: &str, owner_id: &str) -> Result<i64, ShouxinError> {
        self.todo_list_store.count_pending_todos(owner_type, owner_id).await
    }

    /// 获取所有有 pending 待办的 owner
    pub async fn get_all_todo_owners(&self) -> Result<Vec<(String, String)>, ShouxinError> {
        self.todo_list_store.get_all_todo_owners().await
    }

    /// 检查是否已有当天提醒
    pub async fn has_recent_reminder(&self, owner_type: &str, owner_id: &str) -> Result<bool, ShouxinError> {
        self.todo_list_store.has_recent_reminder(owner_type, owner_id).await
    }

    /// 归档陈旧待办（`after_days` 天内未更新者），并最多保留 `keep_count` 条 pending
    pub async fn archive_stale_todos(&self, owner_type: &str, owner_id: &str, keep_count: usize, after_days: i64) -> Result<usize, ShouxinError> {
        self.todo_list_store.archive_stale_todos(owner_type, owner_id, keep_count, after_days).await
    }

    /// 将 deadline 已过期的 pending 待办标记为 auto_invalid
    pub async fn invalidate_expired_todos(&self) -> Result<usize, ShouxinError> {
        self.todo_list_store.invalidate_expired_todos().await
    }

    /// 插入提醒待办
    pub async fn insert_reminder_todo(
        &self,
        todo_id: &str,
        owner_type: &str,
        owner_id: &str,
        title: &str,
    ) -> Result<(), ShouxinError> {
        self.todo_list_store.insert_reminder_todo(todo_id, owner_type, owner_id, title).await
    }
}

#[async_trait]
impl RelationalStoreTrait for RelationalStore {
    async fn add_contact_index(&self, contact: TraitContactIndex) -> Result<(), ShouxinError> {
        query(
            r#"
            INSERT OR REPLACE INTO contact_index 
            (id, name, aliases, is_self, created_at, last_interaction_at, related_contact_ids, 
             relationship_tags, relation_weight, background, notes)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&contact.contact_id)
        .bind(&contact.name)
        .bind("") // aliases
        .bind(0) // is_self
        .bind(contact.created_at.to_rfc3339())
        .bind(contact.last_activity.to_rfc3339())
        .bind("") // related_contact_ids
        .bind("") // relationship_tags
        .bind("") // relation_weight
        .bind("") // background
        .bind("") // notes
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_contact_index(&self, contact_id: &str) -> Result<Option<TraitContactIndex>, ShouxinError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, created_at, last_interaction_at
            FROM contact_index
            WHERE id = ?
            "#,
        )
        .bind(contact_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let id: String = r.get("id");
                let name: String = r.get("name");
                let created_at_str: String = r.get("created_at");
                let last_interaction_at_str: String = r.get("last_interaction_at");

                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                    .with_timezone(&Utc);
                let last_activity = DateTime::parse_from_rfc3339(&last_interaction_at_str)
                    .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                    .with_timezone(&Utc);

                let contact = TraitContactIndex {
                    contact_id: id,
                    name,
                    user_id: None,
                    activity_score: 0.0,
                    last_activity,
                    rank: 0,
                    created_at,
                };
                Ok(Some(contact))
            }
            None => Ok(None),
        }
    }

    async fn search_contacts(&self, search_query: &str, limit: usize) -> Result<Vec<TraitContactIndex>, ShouxinError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, created_at, last_interaction_at
            FROM contact_index
            WHERE name LIKE ? AND is_archived = 0
            ORDER BY last_interaction_at DESC
            LIMIT ?
            "#,
        )
        .bind(format!("%{search_query}%"))
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut contacts = Vec::new();
        for row in rows {
            let id: String = row.get("id");
            let name: String = row.get("name");
            let created_at_str: String = row.get("created_at");
            let last_interaction_at_str: String = row.get("last_interaction_at");

            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                .with_timezone(&Utc);
            let last_activity = DateTime::parse_from_rfc3339(&last_interaction_at_str)
                .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                .with_timezone(&Utc);

            let contact = TraitContactIndex {
                contact_id: id,
                name,
                user_id: None,
                activity_score: 0.0,
                last_activity,
                rank: 0,
                created_at,
            };
            contacts.push(contact);
        }

        Ok(contacts)
    }

    async fn update_contact_activity(&self, contact_id: &str, delta: f32) -> Result<(), ShouxinError> {
        // P1-5: 实现真正的活跃度增量更新
        sqlx::query(
            r#"
            UPDATE contact_index
            SET activity_score = activity_score + ?,
                last_interaction_at = ?
            WHERE id = ?
            "#,
        )
        .bind(delta)
        .bind(Utc::now().to_rfc3339())
        .bind(contact_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_active_contacts(&self, _user_id: Option<&str>, limit: usize) -> Result<Vec<TraitContactIndex>, ShouxinError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, created_at, last_interaction_at
            FROM contact_index
            WHERE is_archived = 0
            ORDER BY last_interaction_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut contacts = Vec::new();
        for row in rows {
            let id: String = row.get("id");
            let name: String = row.get("name");
            let created_at_str: String = row.get("created_at");
            let last_interaction_at_str: String = row.get("last_interaction_at");

            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                .with_timezone(&Utc);
            let last_activity = DateTime::parse_from_rfc3339(&last_interaction_at_str)
                .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                .with_timezone(&Utc);

            let contact = TraitContactIndex {
                contact_id: id,
                name,
                user_id: None,
                activity_score: 0.0,
                last_activity,
                rank: 0,
                created_at,
            };
            contacts.push(contact);
        }

        Ok(contacts)
    }

    async fn remove_contact_index(&self, contact_id: &str) -> Result<(), ShouxinError> {
        // 禁止删除用户记录
        if contact_id == "__self__" {
            return Ok(());
        }

        sqlx::query("DELETE FROM contact_index WHERE id = ?")
            .bind(contact_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn add_event_index(&self, event: TraitEventIndex) -> Result<(), ShouxinError> {
        sqlx::query(
            r#"
            INSERT INTO event_index 
            (event_id, contact_ids, owner_id, owner_type, time_range_start, time_range_end, importance, 
             status, created_at, completed_at, summary)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&event.event_id)
        .bind("") // contact_ids
        .bind(event.user_id.unwrap_or_else(|| "__self__".to_string()))
        .bind("User") // owner_type
        .bind(event.event_time.to_rfc3339())
        .bind(event.event_time.to_rfc3339())
        .bind(event.importance)
        .bind("pending") // status
        .bind(event.created_at.to_rfc3339())
        .bind(Option::<String>::None) // completed_at
        .bind("") // summary
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_event_index(&self, event_id: &str) -> Result<Option<TraitEventIndex>, ShouxinError> {
        let row = sqlx::query(
            r#"
            SELECT event_id, time_range_start, created_at, importance
            FROM event_index
            WHERE event_id = ?
            "#,
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let event_id: String = r.get("event_id");
                let time_range_start_str: String = r.get("time_range_start");
                let created_at_str: String = r.get("created_at");
                let importance: f32 = r.get("importance");

                let event_time = DateTime::parse_from_rfc3339(&time_range_start_str)
                    .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                    .with_timezone(&Utc);
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                    .with_timezone(&Utc);

                let event = TraitEventIndex {
                    event_id,
                    title: None,
                    event_type: "unknown".to_string(),
                    user_id: None,
                    agent_id: None,
                    event_time,
                    archive_id: "".to_string(),
                    memory_id: None,
                    importance,
                    created_at,
                };
                Ok(Some(event))
            }
            None => Ok(None),
        }
    }

    #[allow(unused_assignments)]
    async fn search_events(&self, filter: &EventFilter) -> Result<Vec<TraitEventIndex>, ShouxinError> {
        let mut sql = String::from(
            "SELECT event_id, time_range_start, created_at, importance, summary
             FROM event_index WHERE 1=1"
        );
        
        if !filter.include_archived {
            sql.push_str(" AND is_archived = 0");
        }
        
        let mut archived_dates: Option<(String, String)> = None;
        if let Some(ref archived_month) = filter.archived_month {
            let parts: Vec<&str> = archived_month.split('-').collect();
            if parts.len() == 2 {
                let year = parts[0];
                let month = parts[1];
                let next_month = if month == "12" {
                    "01".to_string()
                } else {
                    format!("{:02}", month.parse::<u32>().unwrap_or(1) + 1)
                };
                sql.push_str(" AND archived_at >= ? AND archived_at < ?");
                archived_dates = Some((
                    format!("{}-{}-01", year, month),
                    format!("{}-{}-01", year, next_month),
                ));
            }
        }
        
        if let Some(ref importance) = filter.importance {
            sql.push_str(" AND importance = ?");
        }
        
        if let Some(ref status) = filter.status {
            sql.push_str(" AND status = ?");
        }
        
        if let Some(start) = filter.start_time {
            sql.push_str(" AND time_range_start >= ?");
        }
        
        if let Some(end) = filter.end_time {
            sql.push_str(" AND time_range_start <= ?");
        }
        
        if let Some(ref query) = filter.search_query {
            sql.push_str(" AND (summary LIKE ? OR event_id LIKE ?)");
        }
        
        sql.push_str(" ORDER BY time_range_start DESC LIMIT ?");
        
        let mut q = sqlx::query(&sql);
        let mut param_idx = 0;

        if let Some((start_date, end_date)) = &archived_dates {
            q = q.bind(start_date);
            q = q.bind(end_date);
            param_idx += 2;
        }

        if let Some(ref imp_str) = filter.importance {
            q = q.bind(imp_str);
            param_idx += 1;
        }
        
        if let Some(ref status) = filter.status {
            q = q.bind(status);
            param_idx += 1;
        }
        
        if let Some(start) = filter.start_time {
            q = q.bind(start.to_rfc3339());
            param_idx += 1;
        }
        
        if let Some(end) = filter.end_time {
            q = q.bind(end.to_rfc3339());
            param_idx += 1;
        }
        
        if let Some(ref query) = filter.search_query {
            let pattern = format!("%{}%", query);
            q = q.bind(pattern.clone());
            q = q.bind(pattern);
            param_idx += 2;
        }
        
        q = q.bind(filter.limit as i64);
        
        let rows = q.fetch_all(&self.pool).await?;
        
        let mut events = Vec::new();
        for row in rows {
            let event_id: String = row.get("event_id");
            let time_range_start_str: String = row.get("time_range_start");
            let created_at_str: String = row.get("created_at");
            let summary: String = row.get::<Option<String>, _>("summary").unwrap_or("".to_string());
            
            // 读取 importance 为字符串，然后转换为数值
            let importance_str: String = row.get("importance");
            let importance = match importance_str.as_str() {
                "high" => 0.8,
                "medium" => 0.5,
                "low" => 0.0,
                "short_term" => 0.4,
                "long_term" => 0.7,
                "critical" => 0.9,
                _ => 0.5,
            };
            
            let time_range_start = DateTime::parse_from_rfc3339(&time_range_start_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            
            // Store summary in event_type temporarily
            events.push(TraitEventIndex {
                event_id,
                title: None,
                event_type: summary,
                user_id: None,
                agent_id: None,
                event_time: time_range_start,
                archive_id: "".to_string(),
                memory_id: None,
                importance,
                created_at,
            });
        }
        
        Ok(events)
    }

    async fn mark_event_completed(&self, event_id: &str) -> Result<(), ShouxinError> {
        sqlx::query(
            r#"
            UPDATE event_index
            SET status = ?, completed_at = ?
            WHERE event_id = ?
            "#,
        )
        .bind("completed")
        .bind(Utc::now().to_rfc3339())
        .bind(event_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn cleanup(&self) -> Result<usize, ShouxinError> {
        Ok(0)
    }
}

impl RelationalStore {
    pub async fn find_contact_by_any_name(
        &self,
        name: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<Option<crate::core::ContactIndex>, ShouxinError> {
        // 注意：**不能**过滤 is_archived = 0。
        //
        // `contact_index` 上有唯一索引 (name, owner_id, owner_type)，归档行同样占用名字。
        // 如果这里跳过归档行，写入方会给同名联系人分配一个新 id，
        // 随后 INSERT 直接撞唯一索引，导致**整条对话抽取失败**（contacts/events/todos 全丢）。
        // 200 条压测实测：活跃联系人超过 50 上限被归档后，
        // 后续提到这些人的对话 100% 失败（错误码 2067 UNIQUE constraint failed）。
        // 正确语义是：老联系人被重新提到 → 找到它 → 复活（见 MemoryWriter::build_contact_index）。
        let rows = query(
            r#"
            SELECT id, name, aliases, is_self, created_at, last_interaction_at,
                   related_contact_ids, relationship_tags, relation_weight, background, notes,
                   owner_type, owner_id, is_agent, parent_agent_id, is_virtual, is_archived, archived_at, activity_score
            FROM contact_index
            WHERE (name = ? OR aliases LIKE ?) AND owner_type = ? AND owner_id = ?
            ORDER BY is_archived ASC, last_interaction_at DESC
            LIMIT 1
            "#,
        )
        .bind(name)
        .bind(format!("%\"{}\"%", name))
        .bind(format!("{:?}", owner_type))
        .bind(owner_id)
        .fetch_optional(&self.pool)
        .await?;

        match rows {
            Some(row) => Ok(Some(crate::core::ContactIndex::from_row(&row)?)),
            None => Ok(None),
        }
    }

    pub async fn get_processing_state(&self) -> Result<crate::heartbeat::types::ProcessingState, ShouxinError> {
        let row = query(
            r#"
            SELECT last_processed_ts, last_profile_update_ts, heartbeat_count, consecutive_skips
            FROM memory_processing_state
            WHERE id = 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let last_processed_str: String = r.get("last_processed_ts");
                let last_profile_str: String = r.get("last_profile_update_ts");
                Ok(crate::heartbeat::types::ProcessingState {
                    last_processed_ts: DateTime::parse_from_rfc3339(&last_processed_str)
                        .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                        .with_timezone(&Utc),
                    last_profile_update_ts: DateTime::parse_from_rfc3339(&last_profile_str)
                        .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                        .with_timezone(&Utc),
                    heartbeat_count: r.get("heartbeat_count"),
                    consecutive_skips: r.get("consecutive_skips"),
                })
            }
            None => Ok(crate::heartbeat::types::ProcessingState {
                last_processed_ts: DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                last_profile_update_ts: DateTime::parse_from_rfc3339("1970-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                heartbeat_count: 0,
                consecutive_skips: 0,
            }),
        }
    }

    pub async fn update_processing_state(&self, last_processed_ts: DateTime<Utc>) -> Result<(), ShouxinError> {
        query(
            r#"
            UPDATE memory_processing_state
            SET last_processed_ts = ?, heartbeat_count = heartbeat_count + 1, updated_at = ?
            WHERE id = 1
            "#,
        )
        .bind(last_processed_ts.to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_pending_ambiguities(&self) -> Result<Vec<crate::heartbeat::types::AmbiguityInfo>, ShouxinError> {
        let rows = query(
            r#"
            SELECT id, ambiguity_type, entity_type, entity_id, description, context_summary,
                   resolved_todo_id, status
            FROM ambiguity_log
            WHERE status = 'pending'
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut ambiguities = Vec::new();
        for row in rows {
            let ambiguity_type_str: String = row.get("ambiguity_type");
            let entity_type_str: String = row.get("entity_type");

            ambiguities.push(crate::heartbeat::types::AmbiguityInfo {
                ambiguity_id: row.get::<i64, _>("id").to_string(),
                ambiguity_type: match ambiguity_type_str.as_str() {
                    "FactualContradiction" => crate::heartbeat::types::AmbiguityType::FactualContradiction,
                    "PronounAmbiguity" => crate::heartbeat::types::AmbiguityType::PronounAmbiguity,
                    "RelationUncertain" => crate::heartbeat::types::AmbiguityType::RelationUncertain,
                    _ => crate::heartbeat::types::AmbiguityType::UncertainInformation,
                },
                entity_type: match entity_type_str.as_str() {
                    "Contact" => crate::heartbeat::types::EntityType::Contact,
                    _ => crate::heartbeat::types::EntityType::Event,
                },
                entity_id: row.get("entity_id"),
                description: row.get("description"),
                context_summary: row.get::<Option<String>, _>("context_summary").unwrap_or_default(),
                conflicting_statements: Vec::new(),
                related_contact_ids: Vec::new(),
                related_event_ids: Vec::new(),
                todo_id: row.get("resolved_todo_id"),
                status: crate::heartbeat::types::AmbiguityStatus::Pending,
            });
        }

        Ok(ambiguities)
    }

    /// 挑出本轮该生成/更新画像的联系人
    ///
    /// 语义（与产品约定一致，不是"谁空就补谁"）：
    /// 1. **没有画像的必须补上**，不看"最近 7 天有没有互动"——
    ///    旧实现带了 `last_interaction_at >= date('now','-7 days')`，
    ///    导致老联系人的画像空缺被永久跳过（用户明确要求"不能漏掉"）。
    /// 2. **有画像但今天又被提到**（last_interaction_at > 上次画像时间）→ 更新，
    ///    这就是"早一次晚一次"的来源。
    /// 3. **今天完全没被提到**（没有新互动）→ 不选，不消耗资源、不计尝试次数。
    /// 4. `max_updates_per_day` 只是**资源节奏**（当天最多跑几次），
    ///    不是"这个联系人只能更新两次"；用完就明天继续，空缺不会被吞掉。
    ///
    /// 排序：先补空缺（从没画像的），再按最近互动时间排；
    /// 已归档的放最后（仍会被补，但优先级最低）。
    pub async fn get_contacts_needing_profile(
        &self,
        max_updates_per_day: u32,
    ) -> Result<Vec<crate::core::ContactIndex>, ShouxinError> {
        let rows = query(
            r#"
            SELECT ci.id, ci.name, ci.aliases, ci.is_self, ci.created_at, ci.last_interaction_at,
                   ci.related_contact_ids, ci.relationship_tags, ci.relation_weight,
                   ci.background, ci.notes, ci.owner_type, ci.owner_id, ci.is_agent,
                   ci.parent_agent_id, ci.is_virtual, ci.is_archived, ci.archived_at, ci.activity_score
            FROM contact_index ci
            LEFT JOIN (
                SELECT contact_id, MAX(updated_at) AS last_update
                FROM contact_profile_update_log
                GROUP BY contact_id
            ) l ON l.contact_id = ci.id
            WHERE ci.is_self = 0 AND ci.is_agent = 0 AND ci.is_virtual = 0
              -- 垃圾名字不选：它们永远解析不出画像，却会占住前面几个名额
              AND length(ci.name) BETWEEN 2 AND 8
              AND ci.name NOT LIKE '%的%'
              AND ci.name NOT LIKE '%和%'
              -- 今天用掉的额度（只是节奏控制，不是永久上限）
              AND NOT EXISTS (
                  SELECT 1 FROM contact_profile_update_log t
                  WHERE t.contact_id = ci.id
                    AND substr(t.updated_at, 1, 10) = date('now')
                  GROUP BY t.contact_id
                  HAVING SUM(t.update_count_today) >= ?1
              )
              AND (
                    -- (1) 从没生成过画像：一定要补（不限时间窗口）
                    ci.background IS NULL OR ci.background = ''
                    -- (2) 有新互动：最近一次互动晚于上次画像时间
                    OR (l.last_update IS NOT NULL AND ci.last_interaction_at > l.last_update)
              )
            ORDER BY
              -- 先补空缺，再按互动新近程度；归档的放最后
              CASE WHEN ci.background IS NULL OR ci.background = '' THEN 0 ELSE 1 END,
              ci.is_archived ASC,
              ci.last_interaction_at DESC
            LIMIT 3
            "#,
        )
        .bind(max_updates_per_day as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut contacts = Vec::new();
        for row in rows {
            contacts.push(crate::core::ContactIndex::from_row(&row)?);
        }

        Ok(contacts)
    }

    /// 获取近期需要合并检查的 pending 事件
    pub async fn get_recent_pending_events_for_merge(&self, limit: u32) -> Result<Vec<crate::core::EventIndex>, ShouxinError> {
        let rows = query(
            r#"
            SELECT event_id, contact_ids, owner_id, owner_type, time_range_start, time_range_end,
                   importance, status, created_at, completed_at, summary, title, event_type,
                   deadline, auto_invalid_at, is_auto_invalid, is_merged, merge_target_id,
                   last_merge_check_at, event_time, importance_score, is_archived, archived_at
            FROM event_index
            WHERE status = 'pending' AND is_merged = 0 AND is_archived = 0
            ORDER BY COALESCE(last_merge_check_at, '1970-01-01') ASC, created_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();
        for row in rows {
            events.push(crate::core::EventIndex::from_row(&row)?);
        }
        Ok(events)
    }

    /// 更新事件的合并检查时间戳
    pub async fn update_event_merge_check_time(&self, event_id: &str) -> Result<(), ShouxinError> {
        query("UPDATE event_index SET last_merge_check_at = ? WHERE event_id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(event_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_profile_update_log(&self, contact_id: &str) -> Result<Vec<crate::heartbeat::types::ProfileUpdateLog>, ShouxinError> {
        let rows = query(
            r#"
            SELECT contact_id, updated_at, update_count_today
            FROM contact_profile_update_log
            WHERE contact_id = ? AND updated_at >= date('now', '-1 day')
            ORDER BY updated_at DESC
            "#,
        )
        .bind(contact_id)
        .fetch_all(&self.pool)
        .await?;

        let mut logs = Vec::new();
        for row in rows {
            let updated_at_str: String = row.get("updated_at");
            logs.push(crate::heartbeat::types::ProfileUpdateLog {
                contact_id: row.get("contact_id"),
                date: updated_at_str.split('T').next().unwrap_or("").to_string(),
                update_count: row.get("update_count_today"),
                last_update: DateTime::parse_from_rfc3339(&updated_at_str)
                    .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                    .with_timezone(&Utc),
            });
        }

        Ok(logs)
    }

    pub async fn record_profile_update(&self, contact_id: &str) -> Result<(), ShouxinError> {
        let now = Utc::now().to_rfc3339();

        let result = query(
            r#"
            INSERT INTO contact_profile_update_log (contact_id, updated_at, update_count_today)
            VALUES (?, ?, 1)
            ON CONFLICT(contact_id, updated_at) DO UPDATE SET
            update_count_today = update_count_today + 1
            "#,
        )
        .bind(contact_id)
        .bind(now)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            if !e.to_string().contains("UNIQUE constraint failed") {
                return Err(ShouxinError::StorageError(e.to_string()));
            }
        }

        Ok(())
    }

    pub async fn find_contact_merge_candidates(&self) -> Result<Vec<crate::heartbeat::types::ContactMergeCandidate>, ShouxinError> {
        let rows = query(
            r#"
            SELECT a1.contact_id as contact1_id, a1.alias as alias1,
                   a2.contact_id as contact2_id, a2.alias as alias2,
                   c1.name as name1, c2.name as name2,
                   a1.owner_type, a1.owner_id
            FROM contact_alias_index a1
            JOIN contact_alias_index a2 ON a1.alias = a2.alias AND a1.contact_id < a2.contact_id
            JOIN contact_index c1 ON a1.contact_id = c1.id
            JOIN contact_index c2 ON a2.contact_id = c2.id
            WHERE a1.owner_type = a2.owner_type AND a1.owner_id = a2.owner_id
            AND c1.is_self = 0 AND c2.is_self = 0
            AND c1.is_archived = 0 AND c2.is_archived = 0
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut candidates = Vec::new();
        let mut seen_pairs: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();

        for row in rows {
            let contact1_id: String = row.get("contact1_id");
            let contact2_id: String = row.get("contact2_id");
            let alias: String = row.get("alias1");
            let name1: String = row.get("name1");
            let name2: String = row.get("name2");

            let pair_key = (contact1_id.clone(), contact2_id.clone());
            if seen_pairs.contains(&pair_key) {
                continue;
            }
            seen_pairs.insert(pair_key);

            let shared = vec![alias];
            let confidence = 0.85;

            candidates.push(crate::heartbeat::types::ContactMergeCandidate {
                primary_contact_id: contact1_id,
                merged_contact_id: contact2_id,
                shared_aliases: shared,
                shared_event_ratio: 0.0,
                confidence_score: confidence,
                merge_type: crate::heartbeat::types::MergeType::AliasOverlap,
            });
        }

        Ok(candidates)
    }

    pub async fn merge_contacts(&self, primary_id: &str, merged_id: &str) -> Result<(), ShouxinError> {
        let confidence = 1.0;

        query(
            r#"
            INSERT INTO contact_merge_log (primary_contact_id, merged_contact_id, merge_type, confidence_score, user_confirmed)
            VALUES (?, ?, 'auto', ?, 0)
            "#,
        )
        .bind(primary_id)
        .bind(merged_id)
        .bind(confidence)
        .execute(&self.pool)
        .await?;

        let merged_contacts = query(
            r#"
            SELECT aliases FROM contact_index WHERE id = ?
            "#,
        )
        .bind(merged_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = merged_contacts {
            let merged_aliases_str: String = row.get("aliases");
            let merged_aliases: Vec<String> = serde_json::from_str(&merged_aliases_str).unwrap_or_default();

            if !merged_aliases.is_empty() {
                let primary_aliases_str: String = query(
                    r#"SELECT aliases FROM contact_index WHERE id = ?"#,
                )
                .bind(primary_id)
                .fetch_optional(&self.pool)
                .await?
                .map(|r| r.get::<String, _>("aliases"))
                .unwrap_or_else(|| "[]".to_string());

                let mut primary_aliases: Vec<String> = serde_json::from_str(&primary_aliases_str).unwrap_or_default();

                for alias in merged_aliases {
                    if !primary_aliases.contains(&alias) {
                        primary_aliases.push(alias);
                    }
                }

                query(
                    r#"UPDATE contact_index SET aliases = ? WHERE id = ?"#,
                )
                .bind(serde_json::to_string(&primary_aliases).unwrap_or_default())
                .bind(primary_id)
                .execute(&self.pool)
                .await?;
            }
        }

        query(
            r#"
            UPDATE event_index
            SET contact_ids = REPLACE(contact_ids, ?, ?)
            WHERE contact_ids LIKE ?
            "#,
        )
        .bind(format!("\"{}\"", merged_id))
        .bind(format!("\"{}\"", primary_id))
        .bind(format!("%\"{}\"%", merged_id))
        .execute(&self.pool)
        .await?;

        query("DELETE FROM contact_index WHERE id = ?")
            .bind(merged_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn resolve_ambiguity(&self, ambiguity_id_or_todo: &str, answer: &str) -> Result<(), ShouxinError> {
        let ambiguity_id_int: Option<i64> = ambiguity_id_or_todo.parse().ok();
        
        if let Some(id) = ambiguity_id_int {
            query(
                r#"
                UPDATE ambiguity_log
                SET status = 'resolved', resolved_at = ?
                WHERE id = ?
                "#,
            )
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        } else {
            let _ = ambiguity_id_or_todo;
            let _ = answer;
            let rows_affected = sqlx::query(
                r#"
                UPDATE ambiguity_log
                SET status = 'resolved', resolved_at = ?
                WHERE resolved_todo_id = ?
                "#,
            )
            .bind(Utc::now().to_rfc3339())
            .bind(ambiguity_id_or_todo)
            .execute(&self.pool)
            .await?
            .rows_affected();
            
            if rows_affected == 0 {
                debug!("No ambiguity record found for todo_id={}", ambiguity_id_or_todo);
            }
        }

        Ok(())
    }

    pub async fn create_ambiguity_todo(
        &self,
        ambiguity_type: &str,
        entity_type: &str,
        entity_id: &str,
        description: &str,
        context_summary: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<String, ShouxinError> {
        let todo_id = uuid::Uuid::new_v4().to_string();
        let deadline = Utc::now() + chrono::Duration::days(7);

        let ambiguity_id_int: i64 = query(
            r#"
            INSERT INTO ambiguity_log (ambiguity_type, entity_type, entity_id, description, context_summary, status)
            VALUES (?, ?, ?, ?, ?, 'pending')
            "#,
        )
        .bind(ambiguity_type)
        .bind(entity_type)
        .bind(entity_id)
        .bind(description)
        .bind(context_summary)
        .execute(&self.pool)
        .await?
        .last_insert_rowid() as i64;

        let todo = crate::stores::relational::todo_list::TodoItem::new(
            todo_id.clone(),
            vec![entity_id.to_string()],
            owner_id.to_string(),
            owner_type,
            format!("澄清: {}", description),
            deadline,
            "medium".to_string(),
        )
        .with_description(context_summary.to_string());

        self.add_todo(&todo).await?;

        query(
            r#"
            UPDATE ambiguity_log
            SET resolved_todo_id = ?
            WHERE id = ?
            "#,
        )
        .bind(&todo_id)
        .bind(ambiguity_id_int)
        .execute(&self.pool)
        .await?;

        Ok(todo_id)
    }

    pub async fn insert_pending_event_merge(
        &self,
        source_event_id: &str,
        target_event_id: Option<&str>,
        source_event_summary: &str,
        target_event_summary: Option<&str>,
        similarity_score: f32,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<i64, ShouxinError> {
        let id = query(
            r#"
            INSERT INTO pending_event_merge 
            (source_event_id, target_event_id, source_event_summary, target_event_summary, 
             similarity_score, status, owner_type, owner_id)
            VALUES (?, ?, ?, ?, ?, 'pending', ?, ?)
            "#,
        )
        .bind(source_event_id)
        .bind(target_event_id)
        .bind(source_event_summary)
        .bind(target_event_summary)
        .bind(similarity_score)
        .bind(format!("{:?}", owner_type))
        .bind(owner_id)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();

        Ok(id)
    }

    pub async fn get_pending_event_merges(
        &self,
        owner_type: Option<crate::core::contact::OwnerType>,
        owner_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<PendingEventMerge>, ShouxinError> {
        let limit = limit.unwrap_or(50);
        
        let mut sql = String::from(
            r#"
            SELECT id, source_event_id, target_event_id, source_event_summary, target_event_summary,
                   similarity_score, status, created_at, resolved_at, resolution_action,
                   owner_type, owner_id
            FROM pending_event_merge
            WHERE status = 'pending'
            "#,
        );

        let mut params: Vec<String> = Vec::new();

        if let Some(ot) = &owner_type {
            sql.push_str(" AND owner_type = ?");
            params.push(format!("{:?}", ot));
        }
        if let Some(oid) = owner_id {
            sql.push_str(" AND owner_id = ?");
            params.push(oid.to_string());
        }

        sql.push_str(" ORDER BY created_at DESC LIMIT ?");
        params.push(limit.to_string());

        let mut query_builder = sqlx::query(&sql);
        for param in &params {
            query_builder = query_builder.bind(param);
        }

        let rows = query_builder.fetch_all(&self.pool).await?;

        let mut merges = Vec::new();
        for row in rows {
            merges.push(PendingEventMerge {
                id: row.get::<i64, _>("id"),
                source_event_id: row.get("source_event_id"),
                target_event_id: row.get("target_event_id"),
                source_event_summary: row.get("source_event_summary"),
                target_event_summary: row.get("target_event_summary"),
                similarity_score: row.get("similarity_score"),
                status: row.get("status"),
                // SQLite 的 CURRENT_TIMESTAMP 形如 "2026-10-28 13:38:37"（不是 RFC3339），
                // 之前按 RFC3339 解析会报 "premature end of input"。
                created_at: parse_sqlite_datetime(&row.get::<String, _>("created_at"))?,
                resolved_at: row.get::<Option<String>, _>("resolved_at")
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                resolution_action: row.get("resolution_action"),
                owner_type: crate::core::contact::OwnerType::parse(
                    &row.get::<String, _>("owner_type"),
                )
                .unwrap_or(crate::core::contact::OwnerType::User),
                owner_id: row.get("owner_id"),
            });
        }

        Ok(merges)
    }

    pub async fn resolve_pending_event_merge(
        &self,
        merge_id: i64,
        action: &str,
    ) -> Result<(), ShouxinError> {
        match action {
            "merge" | "create" | "dismiss" | "approve" | "reject" => {}
            _ => return Err(ShouxinError::GeneralError(format!("Unknown action: {}", action))),
        }

        query(
            r#"
            UPDATE pending_event_merge
            SET status = 'resolved', resolved_at = ?, resolution_action = ?
            WHERE id = ?
            "#,
        )
        .bind(Utc::now().to_rfc3339())
        .bind(action)
        .bind(merge_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn cleanup_expired_pending_merges(&self) -> Result<usize, ShouxinError> {
        let result = query(
            r#"
            DELETE FROM pending_event_merge
            WHERE status = 'pending' AND created_at < datetime('now', '-48 hours')
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    pub async fn cleanup_expired_events(&self) -> Result<usize, ShouxinError> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let long_term_cutoff = (now - chrono::Duration::days(30)).to_rfc3339();
        let short_term_cutoff = (now - chrono::Duration::days(7)).to_rfc3339();

        // 1. 基于 auto_invalid_at 的精确失效（auto_invalid_at 已在创建时设置）
        let result_auto = sqlx::query(
            r#"
            UPDATE event_index
            SET is_archived = 1, archived_at = ?1, is_auto_invalid = 1
            WHERE is_archived = 0
            AND auto_invalid_at IS NOT NULL
            AND auto_invalid_at < ?2
            "#,
        )
        .bind(&now_str)
        .bind(&now_str)
        .execute(&self.pool)
        .await?;

        // 2. 基于 importance_score + created_at 的兜底归档（兼容旧数据无 auto_invalid_at）
        let result_fallback = sqlx::query(
            r#"
            UPDATE event_index
            SET is_archived = 1, archived_at = ?, is_auto_invalid = 1
            WHERE is_archived = 0
            AND (
                (importance_score > 0.7 AND created_at < ?)
                OR
                (importance_score <= 0.7 AND importance_score > 0.4 AND created_at < ?)
            )
            "#,
        )
        .bind(&now_str)
        .bind(&long_term_cutoff)
        .bind(&short_term_cutoff)
        .execute(&self.pool)
        .await?;

        let total = result_auto.rows_affected() + result_fallback.rows_affected();
        tracing::info!(
            "[Event Lifecycle] Archived {} expired events (auto_invalid_at: {}, fallback: {})",
            total, result_auto.rows_affected(), result_fallback.rows_affected()
        );

        Ok(total as usize)
    }

    /// 归档热表中最旧的事件（当热表满时）
    ///
    /// 根据 owner_type/owner_id 隔离，按重要性分数区分长期/短期事件，
    /// 当对应分类的热表事件数超过上限时，归档最旧的事件。
    pub async fn archive_oldest_hot_event(
        &self,
        owner_type: &str,
        owner_id: &str,
        is_long_term: bool,
        hot_limit: usize,
    ) -> Result<(), ShouxinError> {
        let importance_threshold: f32 = if is_long_term { 0.7 } else { 0.0 };
        let importance_max: f32 = if is_long_term { 1.1 } else { 0.7 };

        // Count current hot events
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM event_index WHERE owner_type = ?1 AND owner_id = ?2 AND is_archived = 0 AND importance_score > ?3 AND importance_score <= ?4"
        )
        .bind(owner_type)
        .bind(owner_id)
        .bind(importance_threshold)
        .bind(importance_max)
        .fetch_one(&self.pool)
        .await?;

        if (count.0 as usize) < hot_limit {
            return Ok(());
        }

        // Find oldest events to archive (archive enough to get back to limit)
        let to_archive = (count.0 as usize) - hot_limit;

        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT event_id FROM event_index WHERE owner_type = ?1 AND owner_id = ?2 AND is_archived = 0 AND importance_score > ?3 AND importance_score <= ?4 ORDER BY created_at ASC LIMIT ?5"
        )
        .bind(owner_type)
        .bind(owner_id)
        .bind(importance_threshold)
        .bind(importance_max)
        .bind(to_archive as i64)
        .fetch_all(&self.pool)
        .await?;

        let now = chrono::Utc::now().to_rfc3339();
        for (event_id,) in &rows {
            sqlx::query(
                "UPDATE event_index SET is_archived = 1, archived_at = ?1 WHERE event_id = ?2"
            )
            .bind(&now)
            .bind(event_id)
            .execute(&self.pool)
            .await?;
            tracing::info!("Archived event {} to cold storage (hot limit reached)", event_id);
        }

        Ok(())
    }

    pub async fn cleanup_low_importance_events(&self) -> Result<usize, ShouxinError> {
        let result = sqlx::query(
            r#"
            DELETE FROM event_index
            WHERE importance_score < 0.4 AND is_archived = 0
            "#,
        )
        .execute(&self.pool)
        .await?;

        tracing::info!(
            "[Event Lifecycle] Deleted {} low importance events (< 0.4)",
            result.rows_affected()
        );

        Ok(result.rows_affected() as usize)
    }

    /// 记忆提升/降级：基于事件在近期对话中被引用的次数调整 importance_score
    /// - Promote: 短期记忆(0.4-0.7)在最近7天被3+个新事件共享 contact_ids → 提升至 0.75
    /// - Demote: 长期记忆(>0.7)在最近60天无任何新事件共享 contact_ids → 降至 0.65
    pub async fn promote_demote_events(&self) -> Result<(usize, usize), ShouxinError> {
        let now = Utc::now();
        let seven_days_ago = (now - chrono::Duration::days(7)).to_rfc3339();
        let sixty_days_ago = (now - chrono::Duration::days(60)).to_rfc3339();

        // Promote: 短期记忆被近期事件频繁引用（共享 contact_ids）
        let promote_result = sqlx::query(
            r#"
            UPDATE event_index
            SET importance_score = 0.75
            WHERE is_archived = 0
            AND importance_score > 0.4 AND importance_score <= 0.7
            AND event_id IN (
                SELECT DISTINCT e1.event_id
                FROM event_index e1
                JOIN event_index e2 ON e1.contact_ids = e2.contact_ids
                   AND e1.contact_ids != '[]' AND e1.contact_ids != ''
                   AND e1.event_id != e2.event_id
                WHERE e2.created_at > ?1
                GROUP BY e1.event_id
                HAVING COUNT(DISTINCT e2.event_id) >= 3
            )
            "#,
        )
        .bind(&seven_days_ago)
        .execute(&self.pool)
        .await?;

        // Demote: 长期记忆在60天内无新事件共享 contact_ids
        let demote_result = sqlx::query(
            r#"
            UPDATE event_index
            SET importance_score = 0.65
            WHERE is_archived = 0
            AND importance_score > 0.7 AND importance_score < 0.95
            AND created_at < ?1
            AND event_id NOT IN (
                SELECT DISTINCT e1.event_id
                FROM event_index e1
                JOIN event_index e2 ON e1.contact_ids = e2.contact_ids
                   AND e1.contact_ids != '[]' AND e1.contact_ids != ''
                   AND e1.event_id != e2.event_id
                WHERE e2.created_at > ?2
            )
            "#,
        )
        .bind(&sixty_days_ago)
        .bind(&sixty_days_ago)
        .execute(&self.pool)
        .await?;

        let promoted = promote_result.rows_affected() as usize;
        let demoted = demote_result.rows_affected() as usize;
        if promoted > 0 || demoted > 0 {
            tracing::info!(
                "[Event Lifecycle] Promoted {} events, demoted {} events",
                promoted, demoted
            );
        }

        Ok((promoted, demoted))
    }

    /// 获取所有事件的 event_id 集合（用于向量库孤儿记忆检测）
    pub async fn get_all_event_ids(&self) -> Result<std::collections::HashSet<String>, ShouxinError> {
        let rows = query("SELECT event_id FROM event_index")
            .fetch_all(&self.pool)
            .await?;
        let ids: std::collections::HashSet<String> = rows
            .into_iter()
            .map(|r| r.get::<String, _>("event_id"))
            .collect();
        Ok(ids)
    }

    /// 归档长期未联系的联系人
    ///
    /// 将 180 天以上未交互的联系人标记为已归档。
    /// 每次最多处理 batch_limit 条记录。
    ///
    /// # 参数
    /// - `batch_limit`: 单次处理的最大记录数
    ///
    /// # 返回
    /// 实际归档的联系人数量
    pub async fn archive_stale_contacts(&self, batch_limit: usize) -> Result<usize, ShouxinError> {
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(180)).to_rfc3339();
        let now = chrono::Utc::now().to_rfc3339();

        let result = sqlx::query(
            "UPDATE contact_index SET is_archived = 1, archived_at = ?1 \
             WHERE id IN (SELECT id FROM contact_index \
             WHERE is_archived = 0 AND last_interaction_at IS NOT NULL \
             AND last_interaction_at < ?2 LIMIT ?3)"
        )
        .bind(&now)
        .bind(&cutoff)
        .bind(batch_limit as i64)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    /// P1-4: 按 contact_id 单条归档（热表超限裁剪专用）
    ///
    /// 将指定联系人标记为 is_archived = 1，并写入 archived_at 时间戳。
    /// 与 archive_stale_contacts 的区别：本方法不按时间过滤，直接按 ID 归档，
    /// 供活跃联系人数量超限（MAX_ACTIVE_CONTACTS）时使用，避免物理删除导致
    /// 字典库 ContactTotal 与关系库索引不一致。
    ///
    /// 注意：归档后该联系人不会出现在 get_active_contacts / search_contacts_by_name
    /// 的结果中（两者均过滤 is_archived = 0），但其 contact_alias_index 行仍保留，
    /// 后续若同一别名出现可被 find_contact_merge_candidates 重新识别。
    pub async fn archive_contact_by_id(&self, contact_id: &str) -> Result<(), ShouxinError> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE contact_index SET is_archived = 1, archived_at = ?1 WHERE id = ?2 AND is_archived = 0"
        )
        .bind(&now)
        .bind(contact_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn confirm_pending_event_merge(
        &self,
        merge_id: i64,
        action: &str,
        source_event_summary: Option<&str>,
    ) -> Result<String, ShouxinError> {
        let row = sqlx::query(
            r#"
            SELECT id, source_event_id, target_event_id, source_event_summary, target_event_summary,
                   similarity_score, owner_type, owner_id
            FROM pending_event_merge
            WHERE id = ? AND status = 'pending'
            "#,
        )
        .bind(merge_id)
        .fetch_optional(&self.pool)
        .await?;

        let row = match row {
            Some(r) => r,
            None => return Err(ShouxinError::GeneralError("待确认合并记录不存在或已处理".to_string())),
        };

        let source_event_id: String = row.get("source_event_id");
        let target_event_id: Option<String> = row.get("target_event_id");
        let source_summary: String = row.get("source_event_summary");
        let target_summary: Option<String> = row.get("target_event_summary");
        let owner_type_str: String = row.get("owner_type");
        let owner_id: String = row.get("owner_id");

        let owner_type = match owner_type_str.as_str() {
            "User" => crate::core::contact::OwnerType::User,
            "Contact" => crate::core::contact::OwnerType::Contact,
            "Agent" => crate::core::contact::OwnerType::Agent,
            "Public" => crate::core::contact::OwnerType::Public,
            _ => crate::core::contact::OwnerType::User,
        };

        let event_id = if action == "merge" {
            if let Some(ref target_id) = target_event_id {
                // P1-8: 读取源事件的联系人，合并到目标事件
                let source_contact_ids: Vec<String> = if !source_event_id.starts_with("pending_") {
                    match self.get_event_by_id(&source_event_id).await? {
                        Some(source_event) => {
                            serde_json::from_str(&source_event.contact_ids).unwrap_or_default()
                        }
                        None => {
                            tracing::warn!("[P1-8] 源事件 {} 不存在，跳过联系人合并", source_event_id);
                            Vec::new()
                        }
                    }
                } else {
                    // pending_ 前缀表示源事件未写入 event_index，无联系人可合并
                    Vec::new()
                };

                self.update_event_after_merge(
                    target_id,
                    &source_contact_ids,
                    Some(&source_summary),
                ).await?;

                // P1-8: 标记源事件为已合并（仅当源事件在 event_index 中）
                if !source_event_id.starts_with("pending_") {
                    if let Err(e) = self.update_event_merge_target(&source_event_id, target_id).await {
                        tracing::warn!("[P1-8] 标记源事件 {} 为已合并失败: {}", source_event_id, e);
                    }
                }

                target_id.clone()
            } else {
                return Err(ShouxinError::GeneralError("合并操作需要目标事件ID".to_string()));
            }
        } else {
            let new_event_id = if source_event_id.starts_with("pending_") {
                uuid::Uuid::new_v4().to_string()
            } else {
                source_event_id.clone()
            };

            let summary = source_event_summary.unwrap_or(&source_summary);
            let event_parsed_time = crate::utils::parse_event_time(summary, Utc::now());
            
            let mut event = EventIndex {
                event_id: new_event_id.clone(),
                contact_ids: "[]".to_string(),
                owner_id: owner_id.clone(),
                owner_type,
                time_range_start: Utc::now(),
                time_range_end: Utc::now(),
                importance: "medium".to_string(),
                status: "pending".to_string(),
                created_at: Utc::now(),
                completed_at: None,
                summary: summary.to_string(),
                title: None,
                event_type: String::new(),
                deadline: None,
                auto_invalid_at: None,
                is_auto_invalid: false,
                is_merged: false,
                merge_target_id: None,
                last_merge_check_at: None,
                event_time: event_parsed_time,
                importance_score: 0.5,
                is_archived: false,
                archived_at: None,
            };
            event.compute_auto_invalid_at();

            self.insert_event_index(&event).await?;
            new_event_id
        };

        self.resolve_pending_event_merge(merge_id, action).await?;

        Ok(event_id)
    }

    /// 删除事件索引（**仅用于跨库回滚**）
    ///
    /// 正常业务不应调用：事件的生命周期是"归档/失效"，不是删除。
    /// 这里服务于两阶段提交失败后的补偿清理。
    pub async fn delete_event_by_id(&self, event_id: &str) -> Result<u64, ShouxinError> {
        let result = query("DELETE FROM event_index WHERE event_id = ?")
            .bind(event_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// 删除待确认合并记录（**仅用于跨库回滚**）
    pub async fn delete_pending_event_merge_by_id(
        &self,
        merge_id: i64,
    ) -> Result<u64, ShouxinError> {
        let result = query("DELETE FROM pending_event_merge WHERE id = ?")
            .bind(merge_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn get_event_by_id(
        &self,
        event_id: &str,
    ) -> Result<Option<EventIndex>, ShouxinError> {        let row = query(
            r#"
            SELECT event_id, contact_ids, owner_id, owner_type, time_range_start, time_range_end,
                   importance, status, created_at, completed_at, summary, title, event_type,
                   deadline, auto_invalid_at, is_auto_invalid, is_merged, merge_target_id,
                   last_merge_check_at, event_time, importance_score, is_archived, archived_at
            FROM event_index
            WHERE event_id = ?
            "#,
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(EventIndex::from_row(&r)?)),
            None => Ok(None),
        }
    }

    pub async fn update_event_merge_target(
        &self,
        event_id: &str,
        target_id: &str,
    ) -> Result<(), ShouxinError> {
        query(
            r#"
            UPDATE event_index
            SET is_merged = 1, merge_target_id = ?, last_merge_check_at = ?
            WHERE event_id = ?
            "#,
        )
        .bind(target_id)
        .bind(Utc::now().to_rfc3339())
        .bind(event_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_event_after_merge(
        &self,
        event_id: &str,
        additional_contact_ids: &[String],
        additional_description: Option<&str>,
    ) -> Result<(), ShouxinError> {
        let event = self.get_event_by_id(event_id).await?;

        if let Some(mut event) = event {
            if !additional_contact_ids.is_empty() {
                let existing_contacts: Vec<String> = serde_json::from_str(&event.contact_ids)
                    .unwrap_or_default();
                
                let mut all_contacts = existing_contacts;
                for cid in additional_contact_ids {
                    if !all_contacts.contains(cid) {
                        all_contacts.push(cid.clone());
                    }
                }
                
                event.contact_ids = serde_json::to_string(&all_contacts).unwrap_or_default();
            }

            if let Some(desc) = additional_description {
                if !event.event_type.is_empty() && !desc.is_empty() {
                    event.event_type = format!("{}\n---\n合并自: {}", event.event_type, desc);
                }
            }

            query(
                r#"
                UPDATE event_index
                SET contact_ids = ?, event_type = ?, last_merge_check_at = ?
                WHERE event_id = ?
                "#,
            )
            .bind(&event.contact_ids)
            .bind(&event.event_type)
            .bind(Utc::now().to_rfc3339())
            .bind(event_id)
            .execute(&self.pool)
            .await?;
        } else {
            tracing::warn!(
                "update_event_after_merge: 目标事件 {} 不存在，合并被静默跳过（可能是向量库孤儿记忆）",
                event_id
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingEventMerge {
    pub id: i64,
    pub source_event_id: String,
    pub target_event_id: Option<String>,
    pub source_event_summary: String,
    pub target_event_summary: Option<String>,
    pub similarity_score: f32,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution_action: Option<String>,
    pub owner_type: crate::core::contact::OwnerType,
    pub owner_id: String,
}

/// 解析 SQLite 时间列
///
/// 兼容两种格式：RFC3339（`2026-10-28T13:38:37Z`）与 SQLite `CURRENT_TIMESTAMP`
/// （`2026-10-28 13:38:37`，按 UTC 解释）。
fn parse_sqlite_datetime(value: &str) -> Result<DateTime<Utc>, ShouxinError> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Ok(dt.with_timezone(&Utc));
    }
    let naive = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f"))
        .map_err(|e| ShouxinError::GeneralError(format!("解析时间失败: {} ({})", e, value)))?;
    Ok(chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

/// 联系人的画像相关字段（从 contact_index 单独取出，供检索接口补齐）
#[derive(Debug, Clone, Default)]
pub struct ContactProfileFields {
    /// 别名（JSON 数组字符串）
    pub aliases: Option<String>,
    /// 画像 JSON（个人信息/社会关系/重大事件/评价）
    pub background: Option<String>,
    /// 关系标签（JSON 数组字符串）
    pub relationship_tags: Option<String>,
    /// 关联联系人 ID（JSON 数组字符串）
    pub related_contact_ids: Option<String>,
    /// 是否虚拟人物（智能体扮演的角色）
    pub is_virtual: bool,
}

impl RelationalStore {
    /// 在同一 owner 下按姓名/别名查找联系人 ID
    ///
    /// 用途：把画像里 `social_relations[].related_contact_name`（"张三"）解析成 ID，
    /// 从而把"联系人之间的关系"真正落成可查询的关联。
    /// 限定同一 owner，避免跨用户误关联。
    pub async fn find_contact_id_by_name_same_owner(
        &self,
        anchor_contact_id: &str,
        name: &str,
    ) -> Result<Option<String>, ShouxinError> {
        let pattern = format!("%{}%", name);
        let row = sqlx::query(
            r#"
            SELECT c.id
            FROM contact_index c
            WHERE c.owner_type = (SELECT owner_type FROM contact_index WHERE id = ?1)
              AND c.owner_id   = (SELECT owner_id   FROM contact_index WHERE id = ?1)
              AND c.id <> ?1
              AND (
                    c.name = ?2
                 OR c.name LIKE ?3
                 OR c.aliases LIKE ?3
                 OR EXISTS (
                      SELECT 1 FROM contact_alias_index a
                      WHERE a.contact_id = c.id AND a.alias LIKE ?3
                 )
              )
            LIMIT 1
            "#,
        )
        .bind(anchor_contact_id)
        .bind(name)
        .bind(&pattern)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.get::<String, _>("id")))
    }
}
