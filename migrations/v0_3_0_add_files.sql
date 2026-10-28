-- ShouXin 数据库迁移脚本
-- v0.3.0: 多媒体文件文本化存储
-- 新增 file_index 和 file_relations 表

CREATE TABLE IF NOT EXISTS file_index (
    file_id TEXT PRIMARY KEY,
    file_name TEXT NOT NULL,
    file_type TEXT NOT NULL,
    mime_type TEXT,
    file_size INTEGER,
    md5_hash TEXT,
    owner_type TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    uploaded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    processed_at TIMESTAMP,
    status TEXT DEFAULT 'pending',
    error_message TEXT,
    original_path TEXT,
    extracted_text_summary TEXT
);

CREATE INDEX IF NOT EXISTS idx_file_owner ON file_index(owner_type, owner_id);
CREATE INDEX IF NOT EXISTS idx_file_md5 ON file_index(md5_hash);

CREATE TABLE IF NOT EXISTS file_relations (
    file_id TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    relation_type TEXT DEFAULT 'attached',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (file_id, target_type, target_id)
);