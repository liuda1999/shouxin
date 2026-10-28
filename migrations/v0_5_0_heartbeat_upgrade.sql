-- ShouXin v0.5.0 心跳模块数据库迁移脚本
-- 执行时间：2026-05-30
-- 描述：心跳模块完整重构所需的数据模型升级

-- =====================================================
-- 1. memory_processing_state 表（心跳处理状态）
-- =====================================================
CREATE TABLE IF NOT EXISTS memory_processing_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_processed_ts TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z',
    last_profile_update_ts TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z',
    heartbeat_count INTEGER NOT NULL DEFAULT 0,
    consecutive_skips INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 初始化单行记录
INSERT OR IGNORE INTO memory_processing_state (id, last_processed_ts, last_profile_update_ts)
VALUES (1, '1970-01-01T00:00:00Z', '1970-01-01T00:00:00Z');

-- =====================================================
-- 2. 扩展 event_index 表（事件合并与歧义标记）
-- =====================================================
-- 添加 canonical_event_id：指向主事件的 ID（非空表示当前事件已被合并）
ALTER TABLE event_index ADD COLUMN canonical_event_id TEXT DEFAULT NULL;

-- 添加 merged_from_ids：被合并的事件 ID 列表（JSON 数组）
ALTER TABLE event_index ADD COLUMN merged_from_ids TEXT DEFAULT NULL;

-- 添加 ambiguity_flag：是否存在歧义标记
ALTER TABLE event_index ADD COLUMN ambiguity_flag INTEGER DEFAULT 0;

-- 为新字段创建索引
CREATE INDEX IF NOT EXISTS idx_event_canonical ON event_index(canonical_event_id)
    WHERE canonical_event_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_event_ambiguity ON event_index(ambiguity_flag)
    WHERE ambiguity_flag = 1;

-- =====================================================
-- 3. 扩展 todo_list 表（歧义待办类型）
-- =====================================================
-- 添加 todo_type：normal 或 ambiguity
ALTER TABLE todo_list ADD COLUMN todo_type TEXT NOT NULL DEFAULT 'normal';

-- 添加 entity_type：关联实体类型（contact / event / null）
ALTER TABLE todo_list ADD COLUMN entity_type TEXT DEFAULT NULL;

-- 添加 entity_id：关联实体 ID
ALTER TABLE todo_list ADD COLUMN entity_id TEXT DEFAULT NULL;

-- 添加 resolved_answer：用户回答（歧义待办专用）
ALTER TABLE todo_list ADD COLUMN resolved_answer TEXT DEFAULT NULL;

-- 添加 resolved_at：解决时间
ALTER TABLE todo_list ADD COLUMN resolved_at TEXT DEFAULT NULL;

-- 为新字段创建索引
CREATE INDEX IF NOT EXISTS idx_todo_type ON todo_list(todo_type)
    WHERE todo_type = 'ambiguity';

CREATE INDEX IF NOT EXISTS idx_todo_entity ON todo_list(entity_type, entity_id)
    WHERE entity_type IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_todo_resolved ON todo_list(resolved_at)
    WHERE resolved_at IS NOT NULL;

-- =====================================================
-- 4. 联系人画像更新记录表（用于频率控制）
-- =====================================================
CREATE TABLE IF NOT EXISTS contact_profile_update_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    contact_id TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    update_count_today INTEGER NOT NULL DEFAULT 1,
    UNIQUE(contact_id, updated_at)
);

-- 创建索引用于查询当日更新次数
CREATE INDEX IF NOT EXISTS idx_profile_update_contact ON contact_profile_update_log(contact_id, updated_at);

-- =====================================================
-- 5. 联系人合并记录表
-- =====================================================
CREATE TABLE IF NOT EXISTS contact_merge_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    primary_contact_id TEXT NOT NULL,
    merged_contact_id TEXT NOT NULL,
    merge_type TEXT NOT NULL DEFAULT 'auto',
    confidence_score REAL NOT NULL,
    user_confirmed INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    confirmed_at TEXT DEFAULT NULL,
    FOREIGN KEY (primary_contact_id) REFERENCES contact_index(id),
    FOREIGN KEY (merged_contact_id) REFERENCES contact_index(id)
);

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_merge_primary ON contact_merge_log(primary_contact_id);
CREATE INDEX IF NOT EXISTS idx_merge_merged ON contact_merge_log(merged_contact_id);

-- =====================================================
-- 6. 事件合并记录表
-- =====================================================
CREATE TABLE IF NOT EXISTS event_merge_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    primary_event_id TEXT NOT NULL,
    merged_event_id TEXT NOT NULL,
    merge_reason TEXT NOT NULL,
    time_diff_hours REAL,
    similarity_score REAL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (primary_event_id) REFERENCES event_index(event_id),
    FOREIGN KEY (merged_event_id) REFERENCES event_index(event_id)
);

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_event_merge_primary ON event_merge_log(primary_event_id);

-- =====================================================
-- 7. 歧义检测日志表
-- =====================================================
CREATE TABLE IF NOT EXISTS ambiguity_log (
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
);

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_ambiguity_status ON ambiguity_log(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ambiguity_entity ON ambiguity_log(entity_type, entity_id);

-- =====================================================
-- 8. 联系人别名索引表（用于快速查找）
-- =====================================================
CREATE TABLE IF NOT EXISTS contact_alias_index (
    alias TEXT NOT NULL,
    contact_id TEXT NOT NULL,
    owner_type TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    is_primary INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (alias, contact_id, owner_type, owner_id)
);

-- 创建索引用于按别名查找
CREATE INDEX IF NOT EXISTS idx_alias_lookup ON contact_alias_index(alias, owner_type, owner_id);
CREATE INDEX IF NOT EXISTS idx_contact_aliases ON contact_alias_index(contact_id);

-- =====================================================
-- 迁移完成标记
-- =====================================================
-- 在 schema_migrations 表中记录本次迁移（如果该表存在）
-- 注意：此脚本应与版本控制系统中的迁移记录配合使用
