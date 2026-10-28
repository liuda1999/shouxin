# 强事务 + 统一写入层设计（TRANSACTION_WRITER_DESIGN）

> 日期：2026-10-28
> 依据：`DESIGN_UNDERSTANDING_V2.md`（v2.1 用户确认）
> 目标：`add` 原子落盘+入队；提取结果落三库强事务；同步/异步共用一套写入语义。

---

## 一、两条事务边界

### 边界 A：`add`（原文 + 任务）

- 存储：字典库与 `pending_extraction_tasks` 在**同一个 sled DB**。
- 事务：`(&conversations, &conversation_ts_index, &conversation_id_index, &pending_extraction_tasks).transaction(...)`
- 原子写入：
  1. `ConversationRecord` → `conversations`
  2. `ts_key → conv_key` → `conversation_ts_index`
  3. `conversation_id → conv_key` → `conversation_id_index`
  4. `task_<cid> → ExtractionTask JSON` → `pending_extraction_tasks`
- 结果：要么原文+任务都在，要么都不在；不再依赖"入队失败后扫描恢复"作为主保障。
- 入队到 mpsc 是事务提交后的**内存投递**；投递失败不影响持久化，重启由 `recover_pending_tasks` 重投。

### 边界 B：提取结果提交（三库）

- 事务：关系库（SQLite）+ 向量库 + 字典派生记录。
- 协议：**WAL + 两阶段提交 + 幂等重放 + 可见性控制**。
- 步骤：
  1. sled `commit_log` 写 `tx_id`，`state=prepared`。
  2. 字典库追加派生记录（`ContactTotal` / `ContactEventRecord` 等；append-only，幂等）。
  3. SQLite 开事务，写联系人/事件/待办/文件关系数据（不提交）。
  4. 向量写入 `commit_state=prepared` 的暂存记录（确定性 ID：`tx_id:chunk_index`）。
  5. SQLite commit → 向量标记 `committed` → `commit_log` 置 `committed`。
  6. 失败/崩溃：回滚 SQLite、删除 prepared 向量、`commit_log` 置 `aborted`；字典原文保留，可重试。
- 可见性：搜索只返回 `committed` 的向量与已提交的 SQLite 行；`prepared` 不可见。
- 幂等：`tx_id = conversation_id + ":" + extraction_version`；重放先删同 `tx_id` 的派生记录再写。

---

## 二、统一写入层 MemoryWriter

### 2.1 目标

同步 `ThreeLibraryMemoryManager::add` 与异步 `BackgroundTaskQueue::process_task` 共享同一套写入逻辑，消除 F-M1~F-M5、F-X4/F-X5 的双路径分叉。

### 2.2 接口（提案）

```rust
pub struct MemoryWriteContext {
    pub tx_id: String,
    pub conversation_id: String,
    pub owner_type: OwnerType,
    pub owner_id: String,
}

pub struct MemoryWriter {
    dictionary: Arc<dyn DictionaryStoreTrait>,
    relational: Arc<RelationalStore>,
    vector: Arc<dyn VectorStore>,
    embedder: Arc<dyn Embedder>,
    settings: WriterSettings, // quality_threshold, dedup_threshold, max_active_contacts...
}

impl MemoryWriter {
    pub async fn commit_extraction(
        &self,
        ctx: &MemoryWriteContext,
        elements: &ExtractedElements,
    ) -> Result<(), ShouxinError>;
}
```

### 2.3 职责

1. `prepare`：写 `commit_log`。
2. 字典：`save_contact_total`、`save_contact_event`（保留 aliases/background/notes/tags/related/weight/owner_type）。
3. 关系：SQLite 事务内 `upsert_contact_index`、`insert_event_index`、todo upsert、文件关系。
4. 向量：事件/学习素材带 `tx_id`、`commit_state=prepared` 写入；0.98 去重 + 0.8 质量过滤。
5. `commit` / `abort`：按协议提交或回滚；崩溃由启动恢复处理。

---

## 三、字典 tombstone

- 字典记录禁止物理删除。
- 新增标记字段/树：
  - `ConversationRecord.deleted_at: Option<DateTime<Utc>>`
  - `ContactTotal.deleted_at` / `FileTextDoc.deleted_at`（或统一 `tombstones` 树）
- 删除 API：字典写 tombstone；关系库标记/删除；向量由心跳物理清理。
- 查询：默认过滤 tombstone；审计/恢复场景可显式包含。

---

## 四、热优先 / 冷兜底查询

- 默认只查热表（`is_archived=0`），保证智能体快速取数。
- 热表结果不足时，才以低优先级补查归档（`is_archived=1`），排序在后。
- 不单独分区；只作为查询规则。
- `include_archived` 语义：默认 false（热优先）；显式 true 时才把归档纳入低优先级补充。

---

## 五、恢复与幂等

- 启动时扫描 `commit_log`：
  - `prepared` 且超过阈值：执行 abort（回滚 SQLite 无未提交事务、删 prepared 向量），置 `aborted`，重新入队提取。
  - `committed`：清理日志。
- `recover_pending_tasks` 保留，作为 sled 任务重投。
- 幂等：
  - 关系库按自然键 upsert（contact name+owner、event summary+owner、todo title+owner+deadline）。
  - 向量按 `tx_id:chunk_index` 先删后插。
  - 字典按 `(id, owner)` 幂等写。

---

## 六、测试计划

| 层级 | 用例 |
|------|------|
| 单元 | add 事务：任务写入失败时原文也不落库；成功时两者都在 |
| 单元 | commit：SQLite 失败 → 向量 prepared 被删；向量失败 → SQLite 回滚 |
| 单元 | 崩溃恢复：prepared 日志 → abort + 重新入队 |
| 集成 | 连续 100 条 add → 原文 100、任务 100、处理最终完成 |
| 集成 | 处理中 kill → 重启后 pending 继续，且无部分派生数据可见 |
| 回归 | 同步 `add` 与异步 `add_async` 产生相同的三库结果 |

---

## 七、落地顺序

1. `add` sled 单事务（本阶段）。
2. `MemoryWriter` 骨架 + 字典完整字段写入（修 F-M1/F-M2/F-M3）。
3. SQLite 事务 + 向量 prepared/committed（修 F-M4）。
4. 同步/异步切换到 MemoryWriter（修 F-M5/F-X4/F-X5）。
5. tombstone + 热优先查询。
6. 心跳/配置化/文档。

---

## 八、实施进度（2026-10-28）

| 阶段 | 内容 | 状态 |
|------|------|------|
| 1 | `add` sled 单事务（`save_conversation_with_task` + `pending_extraction_tasks` 同库同事务） | ✅ 完成（含单测 `test_save_conversation_with_task_atomic`） |
| 2 | 字典完整字段写入：`DictionaryStoreTrait` 新增 `get_contact_total` / `patch_contact` / `save_contact_event`，异步路径改用 `ContactPatch` 增量合并；联系人事件历史双路径补齐 | ✅ 完成（F-M1/F-M2/F-M3） |
| 3 | SQLite 事务 + 向量 prepared/committed（F-M4 跨库回滚） | ✅ **已完成**：提交意图状态机 + 进程内补偿（关系库事件/待确认合并 + 向量记忆）+ 启动扫描补偿 |
| 4 | 抽 `MemoryWriter`，同步 `DataDispatcher` 与异步 `write_extraction_results` 合并为单一实现（F-M5） | 🟡 **联系人 + 学习素材已统一**：`write_contacts()`（同步/异步共用）、`write_learnings()`（质量过滤 + 去重 + 向量写入，异步路径已切换，含 2 个单测）；**事件写入未迁**，同步路径的素材写入仍走 `AnyVectorStore` API（需适配器） |
| 5 | 字典 tombstone + 热优先/冷兜底查询 | ⬜ 未开始 |
| 6 | 心跳自适应/配置化 | ✅ 已完成（F-H0/F-H1/F-H2/F-H3；todo 策略见 `[todo]`） |

### 阶段 4 的关键发现（2026-10-28）

清理时确认：**`DataDispatcher::dispatch()` 全代码库没有任何调用方**。
真正在跑的是两套实现：

1. 同步 `ThreeLibraryMemoryManager::add()`（内联实现联系人/事件/待办写入）；
2. 异步 `BackgroundTaskQueue::write_extraction_results()`。

`DataDispatcher` 是第三套、且是**死代码**（仅作为构造参数被传进 manager）。
因此 F-M5 的"双写路径"实际是 1+2 两套，`dispatcher.rs` 可从"参考实现"降级为待删除。
当前 `data_dispatcher` 字段已标注 `#[allow(dead_code)]` 并写明删除条件。

结论：后续只需保证 1、2 共用 `MemoryWriter` 即可完成统一；
`dispatcher.rs` 的 400+ 行可在事件/素材迁移完成后整体删除。


### 阶段 3/4 的过渡状态说明

在阶段 3、4 完成前，异步写入仍是"逐库写入 + 失败即整任务失败"，**没有跨库回滚**：
中途失败会留下部分派生数据（关系库/向量库），字典库因不可删除而始终保留原文。
补偿路径是"字典库原文 → 重新提取"，但当前没有自动补偿任务，需要人工重放。
因此阶段 4 必须先于"生产启用"完成，或至少保证失败任务可重放。

---

## 九、阶段 3 实施规格（跨库回滚，下一步）

### 9.1 已就绪的前置条件

- 追加写序已调整为"关系库 → 向量库 → 字典库"：字典库 append-only、不可回滚，
  必须最后写（`background_queue.rs` 事件路径已按此顺序）。
- 向量库具备 `delete(memory_id)` / `delete_batch(ids)`；`MemoryWriter` 已在写入时持有
  可追踪的 `contact_id` 映射。

### 9.2 需要补的 API

| 位置 | 缺什么 | 状态 |
|------|--------|------|
| `SledDictionaryStore` | `commit_intents` 树 + `begin/record/finish/list_write_intent` | ✅ 已完成（含 2 个单测） |
| `SledDictionaryStore` | 启动扫描陈旧 `prepared` 意图 → `aborted` + 对话置 `failed` | ✅ 已完成（`intent_grace_secs` 可配，默认 300s） |
| `RelationalStore` | `delete_event_by_id(event_id)` | ✅ 已完成 |
| `RelationalStore` | `delete_pending_event_merge_by_id(merge_id)` | ✅ 已完成 |
| `VectorStore` | 已具备 `delete` / `delete_batch` | ✅ 已具备 |

### 9.2.1 当前实现（2026-10-28）

**进程内失败**：`write_extraction_results` 外层捕获错误 →
`compensate_write()` 按 `WriteTracker` 回滚本次新建的事件索引、待确认合并记录、向量记忆 →
意图置 `aborted` 并记录错误。

**进程崩溃**：启动扫描陈旧 `prepared` 意图 → 按意图里登记的
`created_event_ids` / `created_memory_ids` 做同样的补偿 → 意图置 `aborted` →
对话置 `failed`（`needs replay`）。

**已知边界（有意为之）**：
- 字典库不回滚：原文与 `contact_event_history` 保留。回滚掉的派生事件会在重放时重建，
  期间字典里可能短暂存在指向"尚未重建事件"的历史记录——这符合"字典是源文件"的定位。
- **自动合并**（>0.90 合并到已有事件）无法回滚：它修改的是既有事件，没有快照。
  失败时只能保证"不产生新垃圾"，不能撤销已合并的摘要/联系人。
- 补偿本身失败只记日志并保留 `aborted` 意图，由心跳的孤立数据清理兜底。

### 9.3 intent 记录

```
key:   intent:{conversation_id}
value: { "state": "prepared"|"committed"|"aborted",
         "started_at": RFC3339,
         "owner_type": "Agent", "owner_id": "...",
         "created_event_ids": [...], "created_memory_ids": [...] }
```

### 9.4 提交协议（异步写路径）

1. `begin_intent()` 写 `prepared`（含本次 attempt 的唯一 attempt_id）。
2. 关系库写入（SQLite 事务内完成联系人索引/事件/待办）。
3. 向量库写入，记录每个 `memory_id`。
4. 字典库写入联系人事件历史（不可逆，最后）。
5. `commit_intent()` → `committed`。

失败路径：`abort_intent()` → 按 `created_*` 逐项补偿删除（关系库 → 向量库），
字典库不回滚（原文与历史保留，符合"字典永不删除"）。
进程崩溃：启动扫描 `prepared` 且早于 `now - grace` 的 intent，执行同样的补偿，
并把对应对话状态置为 `failed`，等待重放。

### 9.5 不做的事

- 不引入外部事务协调器；不要求 SQLite 与 HNSW 有共享事务。
- 不物理删除字典库任何数据。
- 补偿失败只记录并保留 intent，交由心跳的"孤立数据清理"兜底，不静默吞掉。
