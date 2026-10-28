# ShouXin v0.5.1 - 修复问题2：事件语义合并（embedding 相似度去重）

## 一、问题描述

当前系统存在严重问题：语义相同但措辞不同的事件（如"明天下午3点开项目周会" vs "明天下午3点周会讨论进度"）被重复创建，导致事件表膨胀。

## 二、解决方案

实现基于向量相似度的事件合并机制，使用现有的 `EmbeddedVectorStore` / `HnswVectorStore` 进行相似度计算。

## 三、核心约束

- 必须使用现有向量库进行相似度计算
- 必须实现三种合并策略：自动合并(>0.90)、暂存(0.75-0.90)、创建新(<0.75)
- 不得破坏已有事件查询 API 的兼容性
- 不得使用字符串编辑距离替代 embedding

## 四、实现细节

### 4.1 数据库迁移

创建 `pending_event_merge` 表和新增 `event_index` 字段：

```sql
-- pending_event_merge 表
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

-- event_index 新增字段
ALTER TABLE event_index ADD COLUMN is_merged INTEGER DEFAULT 0;
ALTER TABLE event_index ADD COLUMN merge_target_id TEXT DEFAULT NULL;
ALTER TABLE event_index ADD COLUMN last_merge_check_at TEXT DEFAULT NULL;
```

### 4.2 EventIndex 结构体扩展

```rust
// src/core/contact.rs
pub struct EventIndex {
    // ... 原有字段 ...
    
    /// 是否已合并到其他事件
    pub is_merged: bool,
    /// 合并到的目标事件 ID
    pub merge_target_id: Option<String>,
    /// 最近一次合并检查时间
    pub last_merge_check_at: Option<DateTime<Utc>>,
}
```

### 4.3 语义合并逻辑（ThreeLibraryMemoryManager）

在事件处理流程中集成了相似度判断逻辑：

```rust
// src/memory/three_library_manager.rs

// 生成新事件的 embedding
let event_summary_for_merge = format!(
    "{} {}",
    e.title.as_deref().unwrap_or(""),
    e.description.as_str()
);

if let Some(vector_store) = &self.embedded_vector_store {
    match self.embedder.embed(&event_summary_for_merge).await {
        Ok(new_embedding) => {
            match vector_store.semantic_search_with_embedding(
                &new_embedding,
                3,
                Some(owner_type),
                Some(&owner_id),
            ).await {
                Ok(similar_results) => {
                    if let Some(best_match) = similar_results.first() {
                        let similarity = best_match.score;
                        
                        if similarity > 0.90 {
                            // 自动合并到已有事件
                            tracing::info!(
                                "[Event Merge] 自动合并: '{}' -> '{}' (相似度: {:.4})",
                                event_summary_for_merge, best_match.material_id, similarity
                            );
                            self.relational_store.update_event_after_merge(
                                &best_match.material_id,
                                &e.related_person_ids,
                                Some(&e.description),
                            ).await?;
                            continue;
                        } else if similarity > 0.75 {
                            // 存入待确认合并区
                            tracing::info!(
                                "[Event Merge] 待确认: '{}' 相似度 {:.4} (0.75-0.90)",
                                event_summary_for_merge, similarity
                            );
                            let source_event_id = format!("pending_{}", uuid::Uuid::new_v4());
                            self.relational_store.insert_pending_event_merge(
                                &source_event_id,
                                Some(&best_match.material_id),
                                &event_summary_for_merge,
                                Some(&best_match.content),
                                similarity,
                                owner_type,
                                &owner_id,
                            ).await?;
                            continue;
                        } else {
                            // 创建新事件
                            tracing::debug!(
                                "[Event Merge] 相似度 {:.4} < 0.75，创建新事件",
                                similarity
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("[Event Merge] 向量搜索失败: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::warn!("[Event Merge] Embedding 生成失败: {}", e);
        }
    }
}
```

### 4.4 RelationalStore 新增方法

```rust
// src/stores/relational/store.rs

pub async fn insert_pending_event_merge(
    &self,
    source_event_id: &str,
    target_event_id: Option<&str>,
    source_event_summary: &str,
    target_event_summary: Option<&str>,
    similarity_score: f32,
    owner_type: crate::core::contact::OwnerType,
    owner_id: &str,
) -> Result<i64, ShouxinError>

pub async fn get_pending_event_merges(
    &self,
    owner_type: Option<crate::core::contact::OwnerType>,
    owner_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<PendingEventMerge>, ShouxinError>

pub async fn resolve_pending_event_merge(
    &self,
    merge_id: i64,
    action: &str,
) -> Result<(), ShouxinError>

pub async fn confirm_pending_event_merge(
    &self,
    merge_id: i64,
    action: &str,
    source_event_summary: Option<&str>,
) -> Result<String, ShouxinError>
```

### 4.5 API 端点

新增两个 API 端点：

1. **GET /api/events/pending_merges** - 获取待确认合并列表
2. **POST /api/events/confirm_merge** - 确认或拒绝合并

请求结构：
```rust
pub struct ConfirmMergeRequest {
    pub merge_id: i64,
    pub action: String,  // "merge" 或 "create"
    pub source_event_summary: Option<String>,
}
```

## 五、验证要求

### 5.1 测试用例 E-3（相同语义不同措辞）

1. 第一次输入："明天下午3点开项目周会讨论Q2进度"
   - 预期：创建新事件
   
2. 第二次输入："明天下午3点周会讨论进度"
   - 预期：相似度 >0.90，自动合并

### 5.2 测试用例（不同事件）

1. 第一次输入："明天下午3点开项目周会"
2. 第二次输入："明天下午4点开另一个会"
   - 预期：相似度 <0.75，创建新事件

### 5.3 日志验证

运行时应输出类似日志：
```
[Event Merge] 自动合并: '明天下午3点周会讨论进度' -> 'evt_xxx' (相似度: 0.9456)
[Event Merge] 待确认: '明天下午3点项目会议' 相似度 0.8234 (0.75-0.90)
[Event Merge] 相似度 0.6234 < 0.75，创建新事件
```

## 六、迁移脚本

```sql
-- migrations/v0_5_1_event_semantic_merge.sql

-- 待确认事件合并表
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

CREATE INDEX IF NOT EXISTS idx_pending_event_merge_status ON pending_event_merge(status);
CREATE INDEX IF NOT EXISTS idx_pending_event_merge_owner ON pending_event_merge(owner_type, owner_id);

-- 事件合并字段
ALTER TABLE event_index ADD COLUMN is_merged INTEGER DEFAULT 0;
ALTER TABLE event_index ADD COLUMN merge_target_id TEXT DEFAULT NULL;
ALTER TABLE event_index ADD COLUMN last_merge_check_at TEXT DEFAULT NULL;
```

## 七、注意事项

1. **48小时超时**：待确认事件在 48 小时后自动清理
2. **保守策略**：如果无法解析 embedding，默认为 <0.75 创建新事件
3. **不破坏兼容性**：现有事件查询 API 保持不变
