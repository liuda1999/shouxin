# 记忆管理全链路 · 功能核对报告

> 日期：2026-10-27
> 范围：第 1 条链路「记忆管理」真实调用链
> 方法：静态追踪 + 同步/异步双路径对照 + 字典库 trait 实现核对
> 前置：`cargo check` 通过（两轮）；未启动服务运行

---

## 一、调用链走查

```
POST /api/memory/add
  → routes::add_conversation            (长度/ user_id 校验)
  → AsyncMemory::add_async              (纯委托)
  → ThreeLibraryMemoryManager::add_async
      ├─ dictionary.save_conversation   (原文立即持久化)
      └─ queue.push_with_persist        (sled 持久化 + mpsc 入队)
  → BackgroundTaskQueue::process_task
      ├─ status = processing
      ├─ extractor.extract              (超时/失败 → fallback_extract)
      ├─ write_extraction_results
      │    ├─ dictionary.save_contact   (⚠ 见 F-M1/F-M3)
      │    ├─ relational.upsert_contact_index
      │    ├─ relational.insert_event_index
      │    ├─ vector.insert(event)
      │    ├─ todo_store.add/update
      │    └─ vector.insert(learning)
      └─ status = completed / failed
```

| 环节 | 结论 |
|------|------|
| 入口校验（`MAX_CONVERSATION_LEN`、`validate_user_id`） | ✅ |
| `AsyncMemory` 委托 | ✅ |
| 原文立即落字典库 | ✅ |
| 任务 sled 持久化 + 启动恢复 | ✅ |
| 状态机 `pending → processing → completed/failed` | ✅ |
| LLM 失败/超时降级正则 | ✅ |
| 联系人写关系库（含别名/背景/标签保留） | ✅ |
| 事件写关系库 + 向量库 | ✅ |
| 待办新建/更新意图 | ✅ |
| 学习素材 embedding + 向量写入 | ✅ |
| 字典库联系人字段完整性 | ❌ **F-M1** |
| 字典库联系人事件历史 | ❌ **F-M2** |
| Agent 作用域隔离（字典库） | ❌ **F-M3** |
| 失败回滚/跨库一致性 | ❌ **F-M4** |

---

## 二、功能缺陷清单

### F-M1 🔴 字典库联系人业务字段被异步写回清空

**证据**
- 异步路径：`background_queue.rs:376` 通过 trait `get_contact` 取回 `ContactRecord`，`391` 行用 trait `save_contact(&ContactRecord)` 写回。
- `ContactRecord` 只有 `contact_id/name/user_id/metadata/created_at/updated_at/activity_score`（`store_traits.rs:90-106`），**没有** aliases / background / notes / relationship_tags / related_contact_ids / relation_weight。
- trait 实现在 `store.rs:743-774` 把它转成 `ContactTotal` 时，上述字段全部填 `Vec::new()` / `None`：

```rust
let contact_total = ContactTotal {
    aliases: Vec::new(),
    related_contact_ids: Vec::new(),
    relationship_tags: Vec::new(),
    relation_weight: None,
    background: None,
    notes: None,
    ...
};
```
- 同步路径（`dispatcher.rs:177-190`）用的是**具体** `dictionary_store.get_contact::<ContactTotal>()` + `save_contact(&ContactTotal)`，字段完整保留。

**影响**：默认的 `POST /api/memory/add` 走异步路径，每次写回都会把字典库 `contacts_total` 里的联系人别名、背景、备注、关系标签、关系权重覆盖为空。关系库 `ContactIndex` 暂时保留了这些字段（`background_queue.rs:396-417`），但字典库作为"永久全量存档"已被污染。

**严重度**：🔴 高（数据丢失 + 与"字典库为权威存档"的设计冲突）

---

### F-M2 🔴 字典库缺少 `contact_event_history`（联系人事件历史）

**证据**
- 同步路径 `dispatcher.rs:193-221` 对每个事件的每个关联联系人写 `ContactEventRecord` 到 `dictionary_store.save_contact_event`（sled 树 `contact_event_history`）。
- 异步路径 `background_queue.rs` 全文**没有任何** `save_contact_event` / `ContactEventRecord` 调用（grep 无匹配）。
- 设计文档明确：字典库应存"完整对话文本、联系人总表、**联系人事件历史**"（`FEATURE_MODULES_CN.md` 模块 1 内部架构第 3 点）。

**影响**：v0.5.1 改为异步后，默认写路径丢失联系人事件历史；字典库只剩对话原文 + 联系人总表。

**严重度**：🔴 高（功能缺失 + 文档与实现不符）

---

### F-M3 🔴 Agent 作用域的字典库联系人被写成 `User`

**证据**
- `ContactRecord` 没有 `owner_type` 字段（`store_traits.rs:90-106`）。
- trait 实现 `store.rs:758` 硬编码 `owner_type: OwnerType::User`，同时 `is_agent: false`、`parent_agent_id: None`。
- 异步路径把 `ContactRecord` 写入字典库，不区分 `Scope::User` / `Scope::Agent`；而关系库 `ContactIndex` 用的是真实 `owner_type`（`background_queue.rs:409-410`）。

**影响**：Agent 作用域的联系人进入字典库后变成 `User` 所有，破坏 owner 隔离；后续按 owner 查询/合并时会出现跨作用域串数据。

**严重度**：🔴 高（隔离失效）

---

### F-M4 🟠 异步路径无回滚、无跨库事务

**证据**
- 同步路径 `dispatcher.rs:95-105`：字典写失败会调用 `rollback_dictionary`。
- 异步路径 `write_extraction_results` 顺序写三库，任一步 `?` 返回失败后，`process_task` 仅标 `failed`，**已写入的部分不回滚**。

**影响**：写库中途失败会留下"部分写入 + 状态 failed"的不一致状态；重试/人工修复时难以判断哪些已写。

**严重度**：🟠 中高（一致性）

---

### F-M5 🟠 同步/异步两套写路径重复实现

**证据**
- 同步：`ThreeLibraryMemoryManager::add` → `DataDispatcher::dispatch`（字典 + 关系 + 向量 + 回滚）。
- 异步：`background_queue::write_extraction_results` **重新实现**了一遍联系人/事件/待办/学习写入，未复用 `DataDispatcher`。

**影响**：F-M1/F-M2/F-M3/F-M4 的共同根因；后续任何一边改动都会继续分叉。

**严重度**：🟠 中高（架构债）

---

### F-M6 🟡 状态写回失败被忽略

`process_task` 中 `update_conversation_status(..., "completed", ...)` 用 `let _ =` 忽略错误（`background_queue.rs:279`）。若字典写失败，状态可能永远停在 `processing`。

### F-M7 🟡 `activity_score` 丢失

trait `get_contact` 转换时 `activity_score: 0.0`（`store.rs:792`），异步路径 `contact.update_activity(0.1)` 后写回 ContactRecord；字典库 `ContactTotal` 本身无 activity_score 字段，实际活跃度只存在关系库。属设计层面字段归属不清。

---

## 三、结论

- **高层链路是通的**：入口 → 异步队列 → 提取 → 三库写入 → 状态查询，主流程完整，降级/恢复也有。
- **但默认异步路径的"字典库写入"是有损的**：联系人业务字段被清空、事件历史缺失、Agent 归属被写成 User。这三项都指向同一个根因——异步 worker 自己实现了一套写库逻辑，且用了信息量更少的 `ContactRecord` trait。
- 这三项比之前的文档问题严重得多，建议在"优化升级"之前优先修掉。

---

## 四、修复方案（需你定方向）

| 方案 | 做法 | 优点 | 代价 |
|------|------|------|------|
| **A. 扩展 trait（推荐先做）** | 给 `DictionaryStoreTrait` 增加 `get_contact_total` / `save_contact_total` / `save_contact_event`；异步路径改用这三个方法 | 改动局部、风险低；立刻修好 F-M1/F-M2/F-M3 | 仍保留两套写路径（F-M5 未解） |
| **B. 复用 DataDispatcher** | 把 `Arc<DataDispatcher>` 注入 `WorkerState`，异步 worker 直接调用 `dispatch`（需拆出"只分发、不重复存对话"的入口） | 从根上消除 F-M1~F-M4 | 需要重构 dispatcher 接口，改动中等 |
| **C. 抽 MemoryWriter** | 新建统一写入服务，同步/异步共用 | 架构最干净，利于后续升级 | 改动最大 |

**建议路径**：先 A 止血（本轮可做），把 F-M1/F-M2/F-M3 修掉并补测试；等进入"优化升级"阶段再做 B/C，彻底消除双路径。

---

## 五、下一步

1. 你确认修复方案（A / B / C）。
2. 若选 A，我将：
   - 扩展 `DictionaryStoreTrait` + `SledDictionaryStore` 实现；
   - 异步路径改用 `ContactTotal` 完整字段 + `ContactEventRecord`；
   - 修 `owner_type` 传递；
   - `cargo check` + 针对性测试。
3. 之后再继续第 1 条链路的运行时验证（可选：用旧 exe 做基线冒烟）。
