# ShouXin v0.5.1 - 修复问题4：事件过期清理与热冷分离（E-6）

## 一、问题描述

当前系统无事件过期机制，事件无限增长。需实现：
- LLM 根据事件内容进行重要性打分（0-1）
- 长期事件（得分 >0.7）保留 30 天，短期事件（得分 >0.4）保留 7 天
- 低于 0.4 的不存储（直接丢弃）
- 热表（默认查询）和冷表（归档，按月分页）分离

## 二、解决方案

使用单表 + 状态字段 + 归档策略，在心跳中执行定时清理任务。

## 三、核心约束

- 不得删除原有事件表，使用 `is_archived` 字段区分热冷事件
- 必须使用 LLM 评分
- 必须实现定时清理任务（在心跳中执行）
- 不得硬编码保留天数（应可配置）
- 不得在没有备份的情况下删除事件（归档而非删除）

## 四、实现详情

### 4.1 数据库扩展

```sql
ALTER TABLE event_index ADD COLUMN importance_score REAL DEFAULT 0.5;
ALTER TABLE event_index ADD COLUMN is_archived INTEGER DEFAULT 0;
ALTER TABLE event_index ADD COLUMN archived_at TEXT;
```

### 4.2 EventIndex 结构体扩展

```rust
// src/core/contact.rs
pub struct EventIndex {
    // ... 原有字段 ...
    
    /// LLM 评分的事件重要性（0-1）
    pub importance_score: f32,
    /// 是否已归档（热冷分离）
    pub is_archived: bool,
    /// 归档时间
    pub archived_at: Option<DateTime<Utc>>,
}
```

### 4.3 生命周期规则

| 重要性评分 | 保留期限 | 处理方式 |
|-----------|---------|---------|
| > 0.7 | 30 天 | 归档到冷表 |
| 0.4 - 0.7 | 7 天 | 归档到冷表 |
| < 0.4 | 不存储 | 直接丢弃（不插入） |

### 4.4 心跳清理任务

在 `HeartbeatExecutor::run_heartbeat` 中添加：

```rust
// src/heartbeat/executor.rs

pub async fn run_heartbeat(&self) -> Result<HeartbeatReport, ShouxinError> {
    // ... 原有逻辑 ...
    
    // 清理任务（v0.5.1）
    let _ = self.rel_store.cleanup_expired_pending_merges().await;
    let _ = self.rel_store.cleanup_expired_events().await;
    let _ = self.rel_store.cleanup_low_importance_events().await;
    
    // ... 后续逻辑 ...
}
```

### 4.5 RelationalStore 清理方法

```rust
// src/stores/relational/store.rs

pub async fn cleanup_expired_events(&self) -> Result<usize, ShouxinError> {
    let now = Utc::now();
    let long_term_cutoff = now - chrono::Duration::days(30);
    let short_term_cutoff = now - chrono::Duration::days(7);

    let result = sqlx::query(
        r#"
        UPDATE event_index
        SET is_archived = 1, archived_at = ?
        WHERE is_archived = 0 AND is_auto_invalid = 0
        AND (
            (importance_score > 0.7 AND created_at < ?)
            OR
            (importance_score <= 0.7 AND importance_score > 0.4 AND created_at < ?)
        )
        "#,
    )
    .bind(now.to_rfc3339())
    .bind(long_term_cutoff.to_rfc3339())
    .bind(short_term_cutoff.to_rfc3339())
    .execute(&self.pool)
    .await?;

    tracing::info!(
        "[Event Lifecycle] Archived {} expired events (long_term: 30d, short_term: 7d)",
        result.rows_affected()
    );

    Ok(result.rows_affected() as usize)
}

pub async fn cleanup_low_importance_events(&self) -> Result<usize, ShouxinError> {
    let result = sqlx::query(
        r#"
        DELETE FROM event_index
        WHERE importance_score < 0.4 AND is_auto_invalid = 0 AND is_archived = 0
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
```

### 4.6 API 调整

#### 4.6.1 搜索事件请求扩展

```rust
// src/api/routes.rs
pub struct SearchEventsRequest {
    // ... 原有字段 ...
    
    /// 是否包含已归档事件（热冷分离）
    pub include_archived: bool,
    /// 按归档月份过滤（如 "2026-06"）
    pub archived_month: Option<String>,
}
```

#### 4.6.2 EventFilter 扩展

```rust
// src/traits/store_traits.rs
pub struct EventFilter {
    // ... 原有字段 ...
    
    /// 是否包含已归档事件（热冷分离）
    pub include_archived: bool,
    /// 按归档月份过滤（如 "2026-06"）
    pub archived_month: Option<String>,
}
```

#### 4.6.3 查询逻辑

```rust
// src/stores/relational/store.rs

async fn search_events(&self, filter: &EventFilter) -> Result<Vec<TraitEventIndex>, ShouxinError> {
    let mut sql = String::from(
        "SELECT event_id, time_range_start, created_at, importance, summary
         FROM event_index WHERE 1=1"
    );
    
    // 默认只查询热事件
    if !filter.include_archived {
        sql.push_str(" AND is_archived = 0");
    }
    
    // 按归档月份过滤
    if let Some(ref archived_month) = filter.archived_month {
        let parts: Vec<&str> = archived_month.split('-').collect();
        if parts.len() == 2 {
            let year = parts[0];
            let month = parts[1];
            sql.push_str(&format!(
                " AND archived_at >= '{}-{}-01' AND archived_at < '{}-{}-01'",
                year, month, year, next_month
            ));
        }
    }
    
    // ... 其他过滤条件 ...
}
```

### 4.7 API 使用示例

```bash
# 默认查询（只返回热事件）
POST /api/memory/search/events
{
    "user_id": "user123"
}

# 查询包括归档事件
POST /api/memory/search/events
{
    "user_id": "user123",
    "include_archived": true
}

# 查询指定月份的归档事件
POST /api/memory/search/events
{
    "user_id": "user123",
    "include_archived": true,
    "archived_month": "2026-06"
}
```

## 五、验证要求

### 5.1 长期事件归档测试

1. 创建重要性评分 0.8 的事件
2. 手动修改 `created_at` 为 30 天前
3. 执行心跳
4. 验证：`is_archived = 1`，`archived_at` 已设置

### 5.2 短期事件归档测试

1. 创建重要性评分 0.5 的事件
2. 手动修改 `created_at` 为 7 天前
3. 执行心跳
4. 验证：`is_archived = 1`

### 5.3 低重要性事件丢弃测试

1. 设置重要性评分为 0.3
2. 尝试插入事件
3. 验证：事件不会被插入到数据库

### 5.4 API 测试

```bash
# 默认不返回归档事件
curl -X POST http://localhost:8080/api/memory/search/events \
  -H "Content-Type: application/json" \
  -d '{"user_id": "user123"}'
# 返回结果中不包含 is_archived=true 的事件

# include_archived=true 返回所有事件
curl -X POST http://localhost:8080/api/memory/search/events \
  -H "Content-Type: application/json" \
  -d '{"user_id": "user123", "include_archived": true}'
# 返回所有事件，包括归档的

# 按归档月份过滤
curl -X POST http://localhost:8080/api/memory/search/events \
  -H "Content-Type: application/json" \
  -d '{"user_id": "user123", "include_archived": true, "archived_month": "2026-06"}'
# 只返回 2026 年 6 月归档的事件
```

## 六、迁移脚本

```sql
-- migrations/v0_5_1_event_lifecycle.sql

ALTER TABLE event_index ADD COLUMN importance_score REAL DEFAULT 0.5;
ALTER TABLE event_index ADD COLUMN is_archived INTEGER DEFAULT 0;
ALTER TABLE event_index ADD COLUMN archived_at TEXT;
```

## 七、配置建议

建议在 `config.toml` 中添加可配置的保留天数：

```toml
[event_lifecycle]
long_term_days = 30    # 长期事件保留天数（importance_score > 0.7）
short_term_days = 7     # 短期事件保留天数（0.4 < importance_score <= 0.7）
low_importance_threshold = 0.4  # 低重要性阈值（低于此值不存储）
cleanup_interval_hours = 24     # 清理任务执行间隔（小时）
```

## 八、注意事项

1. **归档而非删除**：所有过期事件都会归档（`is_archived=1`），而不是删除
2. **异步执行**：清理任务在心跳中异步执行，不阻塞正常心跳
3. **日志记录**：每次清理都会记录归档/删除的事件数量
4. **兼容性**：现有 API 默认行为不变，只有显式设置 `include_archived=true` 才返回归档事件
