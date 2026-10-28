# FIX_TODO_HOT_COLD_ARCHIVE.md — 待办热冷分离、上限提醒与自动归档

**任务编号**: 任务7 (T-9 / T-11)  
**版本**: ShouXin v0.5.1  
**状态**: 已完成  
**编译状态**: `cargo build --release --features gguf` 通过 (exit code 0, 仅 39 个预存 warning)

---

## 一、问题描述

当前系统 pending 待办无数量上限，已完成待办无自动清理。需实现热冷分离和自动管理。

## 二、修复方案

### 2.1 三层机制

| 机制 | 触发条件 | 行为 |
|------|---------|------|
| **提醒生成** | pending > 30 | 每日生成一条 `todo_type='reminder'` 的提醒待办 |
| **自动归档** | pending > 50 且 `last_updated_at < 30天` | 归档最旧的待办至 pending ≤ 50 |
| **完成归档** | 状态变为 completed/auto_invalid | 标记 `is_archived=1`（代替原来的 DELETE） |

### 2.2 调度
- 提醒生成和自动归档在每日心跳 (`HeartbeatExecutor::run_heartbeat`) 中执行
- 完成归档在每次 `cleanup()` 调用时执行

---

## 三、代码变更

### 3.1 数据库迁移

```sql
ALTER TABLE todo_list ADD COLUMN is_archived INTEGER DEFAULT 0;
ALTER TABLE todo_list ADD COLUMN archived_at TEXT;
ALTER TABLE todo_list ADD COLUMN last_updated_at TEXT DEFAULT CURRENT_TIMESTAMP;
```

### 3.2 数据结构扩展 — `src/stores/relational/todo_list.rs`

`TodoItem` 新增字段：

```rust
pub struct TodoItem {
    // ... existing fields ...
    /// 是否已归档（热冷分离）
    pub is_archived: bool,
    /// 归档时间
    pub archived_at: Option<DateTime<Utc>>,
    /// 最后更新时间
    pub last_updated_at: DateTime<Utc>,
}
```

### 3.3 新增方法 — `src/stores/relational/todo_list.rs`

#### 统计 pending 待办数

```rust
pub async fn count_pending_todos(&self, owner_type: &str, owner_id: &str) -> Result<i64, ShouxinError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM todo_list 
         WHERE status = 'pending' AND is_archived = 0 
           AND owner_type = ? AND owner_id = ?"
    )
    .bind(owner_type).bind(owner_id)
    .fetch_one(self.pool.as_ref()).await?;
    Ok(row.0)
}
```

#### 获取所有待办所有者

```rust
pub async fn get_all_todo_owners(&self) -> Result<Vec<(String, String)>, ShouxinError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT owner_type, owner_id FROM todo_list 
         WHERE status = 'pending' AND is_archived = 0"
    )
    .fetch_all(self.pool.as_ref()).await?;
    Ok(rows)
}
```

#### 检查当日是否已有提醒

```rust
pub async fn has_recent_reminder(&self, owner_type: &str, owner_id: &str) -> Result<bool, ShouxinError> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM todo_list 
         WHERE owner_type = ? AND owner_id = ? 
           AND todo_type = 'reminder' AND created_at >= ?"
    )
    .bind(owner_type).bind(owner_id).bind(&today)
    .fetch_one(self.pool.as_ref()).await?;
    Ok(row.0 > 0)
}
```

#### 归档过期待办

```rust
pub async fn archive_stale_todos(
    &self, owner_type: &str, owner_id: &str, keep_count: usize
) -> Result<usize, ShouxinError> {
    let count = self.count_pending_todos(owner_type, owner_id).await?;
    if (count as usize) <= keep_count {
        return Ok(0);
    }
    let to_archive = (count as usize) - keep_count;
    let now = chrono::Utc::now().to_rfc3339();
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();

    let result = sqlx::query(
        "UPDATE todo_list SET is_archived = 1, archived_at = ?
         WHERE todo_id IN (
           SELECT todo_id FROM todo_list 
           WHERE status = 'pending' AND is_archived = 0 
             AND owner_type = ? AND owner_id = ? 
             AND last_updated_at < ?
           ORDER BY last_updated_at ASC LIMIT ?
         )"
    )
    .bind(&now).bind(owner_type).bind(owner_id)
    .bind(&cutoff).bind(to_archive as i64)
    .execute(self.pool.as_ref()).await?;

    Ok(result.rows_affected() as usize)
}
```

#### 插入提醒待办

```rust
pub async fn insert_reminder_todo(
    &self, todo_id: &str, owner_type: &str, owner_id: &str, title: &str,
) -> Result<(), ShouxinError> {
    let now = chrono::Utc::now().to_rfc3339();
    let end_of_day = format!("{}T23:59:59Z", chrono::Utc::now().format("%Y-%m-%d"));
    sqlx::query(
        "INSERT INTO todo_list (todo_id, contact_ids, owner_id, owner_type, 
         title, description, deadline, status, created_at, importance, 
         todo_type, is_archived, last_updated_at) 
         VALUES (?, '[]', ?, ?, ?, NULL, ?, 'pending', ?, 'low', 'reminder', 0, ?)"
    )
    .bind(todo_id).bind(owner_id).bind(owner_type)
    .bind(title).bind(&end_of_day)
    .bind(&now).execute(self.pool.as_ref()).await?;
    Ok(())
}
```

#### cleanup 改为归档（非删除）

```rust
pub async fn cleanup(&self) -> Result<usize, ShouxinError> {
    let now = Utc::now().to_rfc3339();
    let result = query(
        r#"UPDATE todo_list 
           SET is_archived = 1, archived_at = ?
           WHERE status IN ('completed', 'auto_invalid') AND is_archived = 0"#
    )
    .bind(&now).execute(self.pool.as_ref()).await?;
    Ok(result.rows_affected() as usize)
}
```

### 3.4 心跳集成 — `src/heartbeat/executor.rs`

在 `run_heartbeat()` 中增加待办管理逻辑：

```rust
// Todo hot/cold management
let owners = self.rel_store.get_all_todo_owners().await.unwrap_or_default();
for (owner_type, owner_id) in &owners {
    let pending_count = self.rel_store.count_pending_todos(owner_type, owner_id).await.unwrap_or(0);

    // Reminder: > 30 pending
    if pending_count > 30 {
        if !self.rel_store.has_recent_reminder(owner_type, owner_id).await.unwrap_or(true) {
            let reminder_id = uuid::Uuid::new_v4().to_string();
            let title = format!("⚠️ 你有 {} 个待办未处理，请及时处理", pending_count);
            if let Err(e) = self.rel_store.insert_reminder_todo(&reminder_id, owner_type, owner_id, &title).await {
                warn!("Failed to insert reminder todo: {}", e);
            } else {
                info!("Generated reminder todo for {}/{}: {} pending", owner_type, owner_id, pending_count);
            }
        }
    }

    // Archive: > 50 pending, archive stale ones
    if pending_count > 50 {
        match self.rel_store.archive_stale_todos(owner_type, owner_id, 50).await {
            Ok(count) => {
                if count > 0 {
                    info!("Archived {} stale todos for {}/{}", count, owner_type, owner_id);
                }
            }
            Err(e) => warn!("Failed to archive stale todos: {}", e),
        }
    }
}
```

### 3.5 查询 API 调整

#### `get_all_todos` — `src/stores/relational/todo_list.rs`

新增 `include_archived` 参数，默认只返回热表（`is_archived=0`）：

```rust
pub async fn get_all_todos(
    &self, owner_type: OwnerType, owner_id: &str,
    status: Option<&str>, limit: i64, include_archived: bool,
) -> Result<Vec<TodoItem>, ShouxinError> {
    let archived_filter = if include_archived { "" } else { " AND is_archived = 0" };
    // ... SELECT with archived_filter ...
}
```

#### API 请求参数 — `src/api/routes.rs`

`ListTodosRequest` 新增 `include_archived` 字段：

```rust
pub struct ListTodosRequest {
    // ... existing fields ...
    #[serde(default)]
    pub include_archived: bool,
}
```

#### API 路由传递 — `src/api/async_memory.rs`

`list_todos()` 透传 `include_archived` 参数到 `TodoListStore::get_all_todos()`。

### 3.6 委托方法 — `src/stores/relational/store.rs`

`RelationalStore` 中新增委托方法：

```rust
pub async fn count_pending_todos(...)       { self.todo_list_store.count_pending_todos(...).await }
pub async fn get_all_todo_owners(...)       { self.todo_list_store.get_all_todo_owners().await }
pub async fn has_recent_reminder(...)       { self.todo_list_store.has_recent_reminder(...).await }
pub async fn archive_stale_todos(...)       { self.todo_list_store.archive_stale_todos(...).await }
pub async fn insert_reminder_todo(...)      { self.todo_list_store.insert_reminder_todo(...).await }
```

---

## 四、测试方案

### 4.1 提醒生成测试

```text
前置条件: 创建 40 个 pending 待办（快速插入）
操作: 等待心跳执行
预期: 生成一条 todo_type='reminder' 的待办，标题包含待办数量
重复执行: 同一天内不再生成第二条提醒
```

### 4.2 自动归档测试

```text
前置条件: 创建 60 个 pending 待办，其中 15 个 last_updated_at 设为 40 天前
操作: 等待心跳执行
预期: 最旧的 ~10 个待办被归档（pending ≤ 50），is_archived=1
```

### 4.3 默认查询测试

```text
操作: GET /api/todos (不传 include_archived)
预期: 只返回 is_archived=0 的待办
```

### 4.4 归档查询测试

```text
操作: GET /api/todos?include_archived=true
预期: 返回所有待办（包含已归档）
```

---

## 五、关键约束检查

| 约束 | 状态 |
|------|------|
| 不得删除待办（仅归档） | OK — cleanup 改为 UPDATE |
| 提醒任务每日最多一次 | OK — `has_recent_reminder` 按日期检查 |
| 归档限制总量 | OK — `archive_stale_todos` 按 keep_count 限定 |
| 分页查询避免大事务 | OK — 使用子查询 `LIMIT` |
| 已完成待办不影响计数 | OK — `count_pending_todos` 过滤 `status='pending'` |
| 归档查询能力保留 | OK — `include_archived=true` |

---

## 六、影响文件

| 文件 | 变更类型 |
|------|---------|
| `src/stores/relational/todo_list.rs` | Schema 扩展 + 5 个新方法 + cleanup 改为归档 |
| `src/stores/relational/store.rs` | 5 个委托方法 |
| `src/heartbeat/executor.rs` | 集成提醒生成和归档逻辑 |
| `src/api/routes.rs` | `ListTodosRequest` 新增 `include_archived` |
| `src/api/async_memory.rs` | 透传 `include_archived` 参数 |