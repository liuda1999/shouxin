-- ShouXin 数据库迁移脚本
-- 从 v0.2.0 升级到 v0.3.0
-- 实现用户与主智能体记忆隔离

-- 为 contact_index 表添加 owner 相关字段
ALTER TABLE contact_index ADD COLUMN owner_type TEXT NOT NULL DEFAULT 'User';
ALTER TABLE contact_index ADD COLUMN owner_id TEXT NOT NULL DEFAULT 'default_user';
ALTER TABLE contact_index ADD COLUMN is_agent INTEGER NOT NULL DEFAULT 0;
ALTER TABLE contact_index ADD COLUMN parent_agent_id TEXT;

-- 更新现有数据的默认值
UPDATE contact_index SET owner_type = 'User', owner_id = 'default_user' WHERE owner_id IS NULL OR owner_id = '';

-- 为 event_index 表添加 owner 字段（已经存在，但确保兼容性）
-- 如果表中没有这些字段，添加它们
-- 注意：event_index 在 v0.2.0 中已经有 owner_id 和 owner_type，但可能需要更新默认值

-- 为 contact_index 添加索引以提高查询性能
CREATE INDEX IF NOT EXISTS idx_contact_owner ON contact_index(owner_type, owner_id);
CREATE INDEX IF NOT EXISTS idx_contact_is_agent ON contact_index(is_agent);

-- 为 event_index 添加索引（如果还没有）
CREATE INDEX IF NOT EXISTS idx_event_owner ON event_index(owner_type, owner_id);

-- 完成迁移
