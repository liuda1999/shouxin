# FIX_EVENT_HOT_LIMIT.md — 事件热表上限与自动归档

**任务编号**: 任务5  
**版本**: ShouXin v0.5.1  
**状态**: 已完成  
**编译状态**: `cargo build --release --features gguf` 通过 (exit code 0, 仅 39 个预存 warning)

---

## 一、问题描述

系统事件表 `event_index` 无限增长，缺乏容量控制。当热表事件数量超过合理上限后，新事件仍然持续写入，导致查询性能下降和存储膨胀。

## 二、修复方案

### 核心逻辑
- 长期事件 (`importance_score > 0.7`)：热表上限 200 条
- 短期事件 (`importance_score ≤ 0.7`)：热表上限 100 条
- 超额时自动归档最早（按 `created_at ASC`）的事件，标记 `is_archived=1`
- 归档在事件插入时**同步执行**，保证热表容量实时有效
- 使用 `owner_type` + `owner_id` 隔离，不同用户/智能体的事件独立计数

---

## 三、代码变更

### 3.1 配置层 — `config.toml`

```toml
# 新增 [event] 配置块
[event]
long_term_hot_limit = 200
short_term_hot_limit = 100
```

### 3.2 配置解析 — `src/config/settings.rs`

新增 `EventSettings` 结构体，含默认值：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSettings {
    pub long_term_hot_limit: usize,
    pub short_term_hot_limit: usize,
}

impl Default for EventSettings {
    fn default() -> Self {
        Self {
            long_term_hot_limit: 200,
            short_term_hot_limit: 100,
        }
    }
}
```

在 `Settings` 结构体中新增字段：

```rust
#[serde(default)]
pub event: EventSettings,
```

### 3.3 数据库迁移 — `src/stores/relational/store.rs`

`event_index` 表已有 `is_archived` (INTEGER DEFAULT 0) 和 `archived_at` (TEXT) 字段（此前已添加）。`insert_event_index` SQL 已包含这两个字段：

```sql
INSERT OR IGNORE INTO event_index 
(event_id, ... importance_score, is_archived, archived_at)
VALUES (?, ..., ?, ?, ?)
```

### 3.4 核心归档方法 — `src/stores/relational/store.rs`

新增 `archive_oldest_hot_event` 方法：

```rust
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
        "SELECT COUNT(*) FROM event_index WHERE owner_type = ?1 AND owner_id = ?2
         AND is_archived = 0 AND importance_score > ?3 AND importance_score <= ?4"
    )
    .bind(owner_type).bind(owner_id)
    .bind(importance_threshold).bind(importance_max)
    .fetch_one(&self.pool).await?;

    if (count.0 as usize) < hot_limit {
        return Ok(());
    }

    // Archive oldest events to get below limit
    let to_archive = (count.0 as usize) - hot_limit + 1;
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT event_id FROM event_index WHERE owner_type = ?1 AND owner_id = ?2
         AND is_archived = 0 AND importance_score > ?3 AND importance_score <= ?4
         ORDER BY created_at ASC LIMIT ?5"
    )
    .bind(owner_type).bind(owner_id)
    .bind(importance_threshold).bind(importance_max)
    .bind(to_archive as i64)
    .fetch_all(&self.pool).await?;

    let now = chrono::Utc::now().to_rfc3339();
    for (event_id,) in &rows {
        sqlx::query(
            "UPDATE event_index SET is_archived = 1, archived_at = ?1 WHERE event_id = ?2"
        )
        .bind(&now).bind(event_id)
        .execute(&self.pool).await?;
        tracing::info!("Archived event {} to cold storage (hot limit reached)", event_id);
    }

    Ok(())
}
```

### 3.5 容量管理集成 — `src/memory/three_library_manager.rs`

在 `ThreeLibraryMemoryConfig` 中新增 `event_settings` 字段：

```rust
pub struct ThreeLibraryMemoryConfig {
    // ... existing fields ...
    pub event_settings: EventSettings,
}
```

在 `add()` 方法的每次事件插入后，调用容量检查：

```rust
// Check hot table capacity
let is_long_term = e.importance > 0.7;
let hot_limit = if is_long_term {
    self.config.event_settings.long_term_hot_limit
} else {
    self.config.event_settings.short_term_hot_limit
};
let owner_type_str = format!("{:?}", owner_type);
self.relational_store.archive_oldest_hot_event(
    &owner_type_str,
    &owner_id,
    is_long_term,
    hot_limit,
).await?;
```

### 3.6 服务器启动 — `src/shouxin-server.rs`

配置传递链：

```rust
let manager_config = ThreeLibraryMemoryConfig::new()
    .with_user_id("default_user".to_string())
    .with_agent_id(settings.agent.primary_agent_id.clone())
    .with_event_settings(settings.event.clone());
```

---

## 四、测试方案

### 4.1 长期事件上限测试

```sql
-- 插入 201 个长期事件（importance_score = 0.8）
-- 第 201 个插入后，最早的事件应被归档

-- 验证热表数量
SELECT COUNT(*) FROM event_index 
WHERE is_archived = 0 AND importance_score > 0.7;
-- 预期结果: <= 200

-- 验证归档事件存在
SELECT COUNT(*) FROM event_index 
WHERE is_archived = 1 AND importance_score > 0.7;
-- 预期结果: >= 1
```

### 4.2 短期事件上限测试

```sql
-- 插入 101 个短期事件（importance_score = 0.5）
-- 第 101 个插入后，最早的事件应被归档

SELECT COUNT(*) FROM event_index 
WHERE is_archived = 0 AND importance_score <= 0.7;
-- 预期结果: <= 100
```

### 4.3 隔离性测试

不同 `owner_type`/`owner_id` 的事件独立计数，互不影响。

### 4.4 编译验证

```powershell
cargo build --release --features gguf
# exit code: 0
# warnings: 39 (均为预存 warning，非本次修改引入)
```

---

## 五、关键约束检查

| 约束 | 状态 |
|------|------|
| 不得删除事件（仅归档） | OK — 使用 `UPDATE SET is_archived=1` |
| 区分长期/短期 (`importance_score > 0.7`) | OK |
| 容量可配置（`config.toml`） | OK |
| 归档在插入时同步执行 | OK |
| owner_type + owner_id 隔离 | OK |

---

## 六、影响文件

| 文件 | 变更类型 |
|------|---------|
| `config.toml` | 新增 `[event]` 配置块 |
| `src/config/settings.rs` | 新增 `EventSettings` 结构体 |
| `src/stores/relational/store.rs` | 新增 `archive_oldest_hot_event` 方法 |
| `src/memory/three_library_manager.rs` | 集成容量管理逻辑 |
| `src/shouxin-server.rs` | 传递 `event_settings` 配置 |