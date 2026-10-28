-- ShouXin v0.3.0 数据库迁移脚本
-- 迁移日期: 2026-05-28
-- 功能: 实现用户和主智能体记忆隔离

-- =======================================================================
-- 1. 为 contact_index 表添加所有者字段
-- =======================================================================
ALTER TABLE contact_index ADD COLUMN owner_type TEXT NOT NULL DEFAULT 'User';
ALTER TABLE contact_index ADD COLUMN owner_id TEXT NOT NULL DEFAULT 'default_user';
ALTER TABLE contact_index ADD COLUMN is_agent BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE contact_index ADD COLUMN parent_agent_id TEXT;

-- 设置现有联系人的默认所有者（假设现有数据属于默认用户）
UPDATE contact_index SET owner_type = 'User', owner_id = 'default_user' WHERE owner_id = 'default_user';

-- 创建所有者索引以提高查询性能
CREATE INDEX IF NOT EXISTS idx_contact_owner ON contact_index(owner_type, owner_id);
CREATE INDEX IF NOT EXISTS idx_contact_is_agent ON contact_index(is_agent);

-- =======================================================================
-- 2. 为 event_index 表添加所有者字段
-- =======================================================================
ALTER TABLE event_index ADD COLUMN owner_type TEXT NOT NULL DEFAULT 'User';
ALTER TABLE event_index ADD COLUMN owner_id TEXT NOT NULL DEFAULT 'default_user';

-- 设置现有事件的默认所有者
UPDATE event_index SET owner_type = 'User', owner_id = 'default_user';

-- 创建所有者索引
CREATE INDEX IF NOT EXISTS idx_event_owner ON event_index(owner_type, owner_id);

-- =======================================================================
-- 3. 待办表 todo_list 已存在 owner_type/owner_id，无需迁移
-- =======================================================================
-- 注意：待办表在之前的版本中已经有了 owner_type 和 owner_id 字段

-- =======================================================================
-- 迁移完成
-- =======================================================================
