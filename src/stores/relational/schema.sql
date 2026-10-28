-- 关系库表结构定义

-- 联系人索引表（活跃前 50）
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
    notes TEXT
);

-- 事件索引表（活跃事件索引）
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
    -- 新增字段：超时自动失效机制
    event_type TEXT,
    deadline TEXT,
    auto_invalid_at TEXT,
    is_auto_invalid INTEGER DEFAULT 0
);

-- 待办事项表（独立待办表）
CREATE TABLE IF NOT EXISTS todo_list (
    todo_id TEXT PRIMARY KEY,
    contact_ids TEXT NOT NULL,
    owner_id TEXT NOT NULL DEFAULT '__self__',
    owner_type TEXT NOT NULL DEFAULT 'User',
    title TEXT NOT NULL,
    description TEXT,
    deadline TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL,
    completed_at TEXT,
    importance TEXT NOT NULL,
    source_event_id TEXT
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_contact_name ON contact_index(name);
CREATE INDEX IF NOT EXISTS idx_contact_last_interaction ON contact_index(last_interaction_at DESC);
CREATE INDEX IF NOT EXISTS idx_contact_is_self ON contact_index(is_self);
CREATE INDEX IF NOT EXISTS idx_event_time_start ON event_index(time_range_start DESC);
CREATE INDEX IF NOT EXISTS idx_event_time_end ON event_index(time_range_end DESC);
CREATE INDEX IF NOT EXISTS idx_event_status ON event_index(status);
CREATE INDEX IF NOT EXISTS idx_event_deadline ON event_index(deadline);
CREATE INDEX IF NOT EXISTS idx_event_auto_invalid ON event_index(is_auto_invalid);
CREATE INDEX IF NOT EXISTS idx_event_owner ON event_index(owner_id, owner_type);
CREATE INDEX IF NOT EXISTS idx_todo_status ON todo_list(status);
CREATE INDEX IF NOT EXISTS idx_todo_deadline ON todo_list(deadline);
CREATE INDEX IF NOT EXISTS idx_todo_owner ON todo_list(owner_id, owner_type);
