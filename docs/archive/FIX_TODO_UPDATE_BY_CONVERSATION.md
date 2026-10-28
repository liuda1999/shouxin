# FIX_TODO_UPDATE_BY_CONVERSATION.md — 对话驱动待办更新

**任务编号**: 任务6  
**版本**: ShouXin v0.5.1  
**状态**: 已完成  
**编译状态**: `cargo build --release --features gguf` 通过 (exit code 0, 仅 39 个预存 warning)

---

## 一、问题描述

系统无法通过对话自动修改待办的截止时间。用户说"把写报告的截止时间改成下周三"时，系统不会自动更新对应待办的 deadline。

## 二、修复方案

### 核心逻辑
1. **Prompt 扩展**：在事件提取的 system prompt 中增加规则3，要求 LLM 同时输出 `todo_update_intent` 字段
2. **意图解析**：LLM 输出中包含 `title_hint`（待办标题关键词）和 `new_deadline`（新截止时间）
3. **模糊匹配**：在 pending 待办中使用 `LIKE '%title_hint%'` 搜索
4. **安全策略**：
   - 唯一匹配 → 自动更新
   - 多个匹配 → 记录警告，不执行
   - 无匹配 → 记录警告，不执行
5. **权限控制**：只匹配用户自己的 pending 待办（`owner_type` + `owner_id` 限定）
6. **无额外 LLM 调用**：复用事件提取的 LLM 响应

---

## 三、代码变更

### 3.1 新增数据结构 — `src/memory_extractor/extractor.rs`

```rust
/// 待办更新意图
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TodoUpdateIntent {
    /// 待办标题提示（用于模糊匹配）
    #[serde(default)]
    pub title_hint: Option<String>,
    /// 新的截止时间（ISO 8601）
    #[serde(default)]
    pub new_deadline: Option<String>,
}
```

在 `ExtractedElements` 中新增字段：

```rust
pub struct ExtractedElements {
    // ... existing fields ...
    /// 待办更新意图
    pub todo_update_intent: Option<TodoUpdateIntent>,
}
```

### 3.2 Prompt 扩展 — `src/memory_extractor/extractor.rs`

在 `default_system_prompt()` 中新增规则3：

```text
3. 修改待办（todo_update_intent）: 如果对话中包含修改待办截止时间的意图，输出待办标题提示和新截止时间
   - title_hint: 待办标题关键词（用于模糊匹配）
   - new_deadline: 新的截止时间，ISO 8601 格式（如"2026-07-03T00:00:00Z"）
   - 没有修改待办意图时设为 null
```

新增 Few-shot 示例：

```json
// 示例6:
{"people":[],"events":[],"todo_update_intent":{"title_hint":"产品需求会","new_deadline":"2026-06-10T00:00:00Z"}}

// 示例7:
{"people":[],"events":[],"todo_update_intent":{"title_hint":"项目进度报告","new_deadline":"2026-07-15T00:00:00Z"}}
```

最终输出格式：

```json
{"people":[...],"events":[...],"todo_update_intent":null}
```

### 3.3 LLM 响应解析 — `src/memory_extractor/extractor.rs`

在 `RawResponse` 解析结构体中新增字段：

```rust
#[derive(serde::Deserialize)]
struct RawResponse {
    // ... existing fields ...
    #[serde(default)]
    todo_update_intent: Option<TodoUpdateIntent>,
}
```

在 `parse_llm_output` 返回时传递：

```rust
Ok(ExtractedElements {
    // ... other fields ...
    todo_update_intent: raw.todo_update_intent,
})
```

### 3.4 模糊搜索方法 — `src/stores/relational/todo_list.rs`

```rust
pub async fn search_pending_todos_by_title_fuzzy(
    &self,
    title_hint: &str,
    owner_type: &str,
    owner_id: &str,
) -> Result<Vec<TodoItem>, ShouxinError> {
    let pattern = format!("%{}%", title_hint);
    let rows = query(
        r#"
        SELECT todo_id, contact_ids, owner_id, owner_type, title, description, 
               deadline, status, created_at, completed_at, importance, 
               source_event_id, todo_type, entity_type, entity_id
        FROM todo_list
        WHERE status = 'pending' AND owner_type = ?1 AND owner_id = ?2 
              AND title LIKE ?3
        "#,
    )
    .bind(owner_type).bind(owner_id).bind(&pattern)
    .fetch_all(self.pool.as_ref()).await?;

    let mut todos = Vec::new();
    for row in rows {
        todos.push(TodoItem::from_row(&row)?);
    }
    Ok(todos)
}
```

### 3.5 截止时间更新方法 — `src/stores/relational/todo_list.rs`

```rust
pub async fn update_todo_deadline(
    &self,
    todo_id: &str,
    new_deadline: &str,
) -> Result<(), ShouxinError> {
    query(
        "UPDATE todo_list SET deadline = ?1 
         WHERE todo_id = ?2 AND status = 'pending'",
    )
    .bind(new_deadline).bind(todo_id)
    .execute(self.pool.as_ref()).await?;
    Ok(())
}
```

### 3.6 意图处理集成 — `src/memory/three_library_manager.rs`

在 `add()` 方法中，事件处理之后增加待办更新逻辑。关键点是**在 `extracted_elements` 被 move 之前先 clone `todo_update_intent`**：

```rust
// Clone before move to avoid borrow error
let todo_update_intent = extracted_elements.todo_update_intent.clone();

// ... event processing ...

// ========== 处理待办更新意图 ==========
if let Some(ref intent) = todo_update_intent {
    if let Some(ref title_hint) = intent.title_hint {
        let owner_type_str = format!("{:?}", owner_type);
        match self.relational_store.get_todo_store()
            .search_pending_todos_by_title_fuzzy(
                title_hint, &owner_type_str, &owner_id,
            ).await {
            Ok(todos) => {
                if todos.len() == 1 {
                    let todo = &todos[0];
                    if let Some(ref new_deadline) = intent.new_deadline {
                        match self.relational_store.get_todo_store()
                            .update_todo_deadline(&todo.todo_id, new_deadline).await {
                            Ok(()) => {
                                tracing::info!(
                                    "Updated todo '{}' deadline to {} based on conversation intent",
                                    todo.title, new_deadline
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Failed to update todo deadline: {}", e);
                            }
                        }
                    }
                } else if todos.is_empty() {
                    tracing::warn!(
                        "Todo update intent: no pending todo matching '{}'",
                        title_hint
                    );
                } else {
                    tracing::warn!(
                        "Todo update intent: multiple ({}) pending todos match '{}', skipping",
                        todos.len(), title_hint
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Todo fuzzy search failed: {}", e);
            }
        }
    }
}
```

### 3.7 后台队列同步 — `src/service/background_queue.rs`

在 `write_extraction_results()` 中同样增加待办更新意图处理：

```rust
// Process todo update intent
if let Some(ref intent) = elements.todo_update_intent {
    if let Some(ref title_hint) = intent.title_hint {
        let owner_type_str = format!("{:?}", task.owner_type);
        match state.relational.get_todo_store()
            .search_pending_todos_by_title_fuzzy(
                title_hint, &owner_type_str, &task.owner_id,
            ).await {
            Ok(todos) => {
                if todos.len() == 1 {
                    // ... update deadline ...
                } else if todos.is_empty() {
                    tracing::warn!("Todo update intent: no match for '{}'", title_hint);
                } else {
                    tracing::warn!("Todo update intent: {} matches, skipping", todos.len());
                }
            }
            Err(e) => { tracing::warn!("Todo fuzzy search failed: {}", e); }
        }
    }
}
```

---

## 四、修复的编译错误

### 错误1: `borrow of moved value`

`extracted_elements.todo_update_intent` 在 `extracted_elements` 被 move 到 `DispatchInput` 后访问。

**修复**: 在 move 之前 clone：

```rust
let todo_update_intent = extracted_elements.todo_update_intent.clone();
```

---

## 五、测试方案

### 5.1 单匹配更新

```text
对话: "把产品需求会的截止时间改成下周三"
前提: 存在一个 pending 待办，标题为 "产品需求会讨论"
预期: 该待办的 deadline 被更新
```

### 5.2 多匹配跳过

```text
对话: "把写报告的截止时间改成7月3日"
前提: 存在两个 pending 待办，"写项目报告" 和 "写周报报告"
预期: 两者都不被修改，日志输出警告 "multiple (2) pending todos match"
```

### 5.3 无匹配跳过

```text
对话: "把不存在的待办的截止时间改了"
前提: 无匹配待办
预期: 不影响任何数据，日志输出警告 "no pending todo matching"
```

### 5.4 编译验证

```powershell
cargo build --release --features gguf
# exit code: 0
```

---

## 六、关键约束检查

| 约束 | 状态 |
|------|------|
| 不得额外增加 LLM 调用 | OK — 复用事件提取响应 |
| 支持模糊匹配 (`LIKE '%...%'`) | OK |
| 更新操作验证所有权 | OK — 限定 `owner_type` + `owner_id` |
| 不得要求用户提供待办 ID | OK |
| 错误匹配需日志记录 | OK |
| 不得修改非 pending 状态待办 | OK |
| 唯一匹配自动更新 / 多匹配跳过 | OK |

---

## 七、影响文件

| 文件 | 变更类型 |
|------|---------|
| `src/memory_extractor/extractor.rs` | 新增 `TodoUpdateIntent` 结构体、扩展 Prompt、扩展解析 |
| `src/stores/relational/todo_list.rs` | 新增 `search_pending_todos_by_title_fuzzy`、`update_todo_deadline` |
| `src/memory/three_library_manager.rs` | 集成待办更新意图处理逻辑 |
| `src/service/background_queue.rs` | 后台队列同步处理待办更新意图 |