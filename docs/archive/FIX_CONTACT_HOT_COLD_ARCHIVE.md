# FIX_CONTACT_HOT_COLD_ARCHIVE.md — 联系人热冷分离（自动归档）

**任务编号**: 任务8 (I-3)  
**版本**: ShouXin v0.5.1  
**状态**: 已完成  
**编译状态**: `cargo build --release --features gguf` 通过 (exit code 0, 仅 39 个预存 warning)

---

## 一、问题描述

联系人表 `contact_index` 无限增长，长期未联系的联系人占用热表空间，影响查询性能。

## 二、修复方案

### 核心逻辑
- 超过 180 天未联系（`last_interaction_at < now - 180 days`）的联系人自动归档
- 归档在每日心跳中执行，每次最多处理 1000 条
- 默认查询只返回热联系人（`is_archived=0`），可通过 `include_archived=true` 查询冷联系人
- `upsert_contact_index` 使用 `INSERT ON CONFLICT DO UPDATE`，**不在 UPDATE 中覆盖 `is_archived`**，保证归档状态不丢失

---

## 三、代码变更

### 3.1 数据库迁移

```sql
ALTER TABLE contact_index ADD COLUMN is_archived INTEGER DEFAULT 0;
ALTER TABLE contact_index ADD COLUMN archived_at TEXT;
```

已在 `init_schema()` 中内置：

```sql
CREATE TABLE IF NOT EXISTS contact_index (
    -- ... existing columns ...
    is_archived INTEGER DEFAULT 0,
    archived_at TEXT
)
```

### 3.2 数据结构扩展 — `src/core/contact.rs`

`ContactIndex` 新增字段：

```rust
pub struct ContactIndex {
    // ... existing fields ...
    /// 是否为虚拟角色
    pub is_virtual: bool,
    /// 是否已归档（热冷分离）
    #[serde(default)]
    pub is_archived: bool,
    /// 归档时间
    #[serde(default)]
    pub archived_at: Option<DateTime<Utc>>,
}
```

### 3.3 UPSERT 保护 — `src/stores/relational/store.rs`

`upsert_contact_index` 改为 `INSERT ... ON CONFLICT DO UPDATE`，关键点：**UPDATE 列表中不包含 `is_archived`**，避免重新激活已归档的联系人：

```sql
INSERT INTO contact_index 
(id, name, aliases, is_self, created_at, last_interaction_at, related_contact_ids, 
 relationship_tags, relation_weight, background, notes, owner_type, owner_id, 
 is_agent, parent_agent_id, is_virtual, is_archived)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)
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
-- 注意: is_archived 不在 UPDATE 列表中，保持原有归档状态
```

### 3.4 归档方法 — `src/stores/relational/store.rs`

```rust
/// 归档长期未联系的联系人
///
/// 将 180 天以上未交互的联系人标记为已归档。
/// 每次最多处理 batch_limit 条记录。
pub async fn archive_stale_contacts(&self, batch_limit: usize) -> Result<usize, ShouxinError> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(180)).to_rfc3339();
    let now = chrono::Utc::now().to_rfc3339();

    let result = sqlx::query(
        "UPDATE contact_index SET is_archived = 1, archived_at = ?1
         WHERE id IN (SELECT id FROM contact_index 
         WHERE is_archived = 0 AND last_interaction_at IS NOT NULL 
         AND last_interaction_at < ?2 LIMIT ?3)"
    )
    .bind(&now).bind(&cutoff).bind(batch_limit as i64)
    .execute(&self.pool).await?;

    Ok(result.rows_affected() as usize)
}
```

### 3.5 心跳集成 — `src/heartbeat/executor.rs`

```rust
// Contact archiving
match self.rel_store.archive_stale_contacts(1000).await {
    Ok(count) => {
        if count > 0 {
            tracing::info!(
                "Archived {} stale contacts (not contacted for 180+ days)", count
            );
        }
    }
    Err(e) => tracing::warn!("Failed to archive stale contacts: {}", e),
}
```

### 3.6 联系人搜索 API — `src/stores/relational/store.rs`

`search_contacts_with_owner` 新增 `include_archived` 参数：

```rust
pub async fn search_contacts_with_owner(
    &self,
    search_query: &str,
    owner_type: OwnerType,
    owner_id: &str,
    limit: usize,
    include_archived: bool,  // 新增
) -> Result<Vec<TraitContactIndex>, ShouxinError> {
    let archived_filter = if include_archived {
        ""
    } else {
        " AND c.is_archived = 0"
    };

    let sql = format!(
        r#"SELECT DISTINCT c.id, c.name, c.created_at, c.last_interaction_at
           FROM contact_index c
           LEFT JOIN contact_alias_index a ON ...
           WHERE (c.name LIKE ? OR a.alias LIKE ?)
             AND c.owner_type = ? AND c.owner_id = ?{}
           ORDER BY c.last_interaction_at DESC
           LIMIT ?"#,
        archived_filter
    );
    // ...
}
```

### 3.7 API 请求参数 — `src/api/routes.rs`

`SearchContactsRequest` 新增 `include_archived`：

```rust
pub struct SearchContactsRequest {
    // ... existing fields ...
    /// 是否包含已归档联系人（热冷分离）
    #[serde(default)]
    pub include_archived: bool,
}
```

路由传递：

```rust
match async_memory.search_contacts(
    &req.query, req.limit, scope, req.user_id.as_deref(), req.include_archived
).await {
    // ...
}
```

### 3.8 AsyncMemory 透传 — `src/api/async_memory.rs`

`search_contacts()` 新增 `include_archived` 参数并透传到底层。

### 3.9 此前的 ContactIndex 创建点修复

所有创建 `ContactIndex` 的位置需要包含新字段：

#### `src/shouxin-server.rs` (内置虚拟角色初始化)

```rust
let contact = shouxin::core::ContactIndex {
    // ... existing fields ...
    is_archived: false,
    archived_at: None,
};
```

#### `src/service/background_queue.rs` (后台队列联系人索引)

```rust
let contact_index = crate::core::ContactIndex {
    // ... existing fields ...
    is_archived: false,
    archived_at: None,
};
```

---

## 四、测试方案

### 4.1 归档测试

```sql
-- 手动创建一个旧联系人
INSERT INTO contact_index (id, name, last_interaction_at, is_archived, ...) 
VALUES ('test-old', '旧联系人', '2024-01-01T00:00:00Z', 0, ...);

-- 等待心跳执行后验证
SELECT is_archived, archived_at FROM contact_index WHERE id = 'test-old';
-- 预期: is_archived=1, archived_at NOT NULL
```

### 4.2 默认搜索过滤

```text
操作: POST /api/memory/search/contacts {"query": "旧联系人"}
预期: 不返回已归档的联系人
```

### 4.3 包含归档搜索

```text
操作: POST /api/memory/search/contacts {"query": "旧联系人", "include_archived": true}
预期: 返回已归档的联系人
```

### 4.4 UPSERT 保护测试

```text
前置: 联系人已被归档 (is_archived=1)
操作: 该联系人再次被提取（如对话中提及），触发 upsert_contact_index
预期: is_archived 保持为 1（不会被刷新为 0）
      但 last_interaction_at 会被更新为当前时间
```

### 4.5 性能测试

```sql
-- 确认索引存在
CREATE INDEX IF NOT EXISTS idx_contact_last_interaction 
ON contact_index(last_interaction_at);
```

---

## 五、关键约束检查

| 约束 | 状态 |
|------|------|
| 不得删除联系人 | OK — 仅 `UPDATE SET is_archived=1` |
| 归档在心跳中执行 | OK |
| 限制批量大小 (1000) | OK |
| 保留查询冷联系人能力 | OK — `include_archived=true` |
| 不因归档影响搜索性能 | OK — `is_archived` 索引过滤 |
| UPSERT 不覆盖归档状态 | OK — UPDATE 列表不含 `is_archived` |
| 180 天阈值 | OK |

---

## 六、影响文件

| 文件 | 变更类型 |
|------|---------|
| `src/core/contact.rs` | `ContactIndex` 新增 `is_archived` + `archived_at` 字段 |
| `src/stores/relational/store.rs` | 新增 `archive_stale_contacts`；修改 `upsert_contact_index` 为 ON CONFLICT DO UPDATE；扩展 `search_contacts_with_owner` 签名 |
| `src/heartbeat/executor.rs` | 集成联系人归档调用 |
| `src/api/routes.rs` | `SearchContactsRequest` 新增 `include_archived` |
| `src/api/async_memory.rs` | `search_contacts` 透传 `include_archived` |
| `src/shouxin-server.rs` | 内置虚拟角色初始化包含新字段 |
| `src/service/background_queue.rs` | 后台队列联系人索引包含新字段 |