-- ShouXin v0.5.1 事件语义合并数据库迁移脚本
-- 执行时间：2026-06-02
-- 描述：基于 embedding 相似度的事件去重机制

-- =====================================================
-- 1. pending_event_merge 表（待确认事件合并）
-- =====================================================
-- 存储语义相似但需要用户确认的事件合并请求
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
);

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_pending_merge_status ON pending_event_merge(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_pending_merge_source ON pending_event_merge(source_event_id);
CREATE INDEX IF NOT EXISTS idx_pending_merge_target ON pending_event_merge(target_event_id);
CREATE INDEX IF NOT EXISTS idx_pending_merge_owner ON pending_event_merge(owner_type, owner_id);

-- =====================================================
-- 2. event_similarity_cache 表（事件相似度缓存）
-- =====================================================
-- 缓存已计算的事件相似度，避免重复计算
CREATE TABLE IF NOT EXISTS event_similarity_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id_1 TEXT NOT NULL,
    event_id_2 TEXT NOT NULL,
    similarity_score REAL NOT NULL,
    calculated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(event_id_1, event_id_2)
);

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_similarity_event1 ON event_similarity_cache(event_id_1);
CREATE INDEX IF NOT EXISTS idx_similarity_event2 ON event_similarity_cache(event_id_2);
CREATE INDEX IF NOT EXISTS idx_similarity_score ON event_similarity_cache(similarity_score DESC);

-- =====================================================
-- 3. 扩展 event_index 表（事件合并辅助字段）
-- =====================================================
-- 添加 is_merged：是否已合并到其他事件
ALTER TABLE event_index ADD COLUMN is_merged INTEGER DEFAULT 0;

-- 添加 merge_target_id：合并到的目标事件 ID
ALTER TABLE event_index ADD COLUMN merge_target_id TEXT DEFAULT NULL;

-- 添加 last_merge_check_at：最近一次合并检查时间
ALTER TABLE event_index ADD COLUMN last_merge_check_at TEXT DEFAULT NULL;

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_event_merged ON event_index(is_merged)
    WHERE is_merged = 1;

CREATE INDEX IF NOT EXISTS idx_event_merge_target ON event_index(merge_target_id)
    WHERE merge_target_id IS NOT NULL;

-- =====================================================
-- 迁移完成标记
-- =====================================================
