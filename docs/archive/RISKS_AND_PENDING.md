# 守心 · 风险登记与待修清单（持续更新）

> 建立日期：2026-10-28
> 最近验证：2026-10-28 · `cargo check --lib --bin shouxin-server` exit 0 ·
> `cargo test --lib` **62 passed / 0 failed** · `test_file_upload` 测试目标可编译
> 用途：记录**下一步开工前必须记住的风险点**，避免修复引入回归；与 `FIX_LIST.md`（问题总账）
> 和 `TRANSACTION_WRITER_DESIGN.md`（事务设计/进度）配套使用。
> 状态图例：🔴 高风险（可能丢数据/破坏隔离）· 🟠 中（行为不一致/静默降级）· 🟡 低（可维护性）

---

## 一、当前架构的真实形状（先记住，避免改错）

代码里其实有 **三套**写库实现，不是两套：

| # | 位置 | 是否在跑 | 向量写入方式 |
|---|------|---------|-------------|
| 1 | `ThreeLibraryMemoryManager::add()`（同步内联） | ✅ 生产在用 | `AnyVectorStore::insert_material_with_embedding` |
| 2 | `BackgroundTaskQueue::write_extraction_results_inner`（异步） | ✅ 生产在用（默认路径） | `VectorStore::insert(Memory)` |
| 3 | `DataDispatcher::dispatch()` | ❌ **零调用方**（死代码，仅作构造参数） | 同 1 |

已统一部分：联系人（`MemoryWriter::write_contacts`）、异步学习素材（`MemoryWriter::write_learnings`）。
**未统一**：事件写入（1 与 2 各一份）、同步学习素材（仍走 1 的 API）。

---

## 二、风险点（修复前必读）

### R1 🟠 两套向量写入 API 语义不同（检索/详情/列表已收敛）

- `insert_material_with_embedding(...)` 把 owner 存在**素材字段**；
- `VectorStore::insert(Memory)` 把 owner 存在 **metadata**。

**已修复（2026-10-28，含复现测试）**：
1. **F-VS6**：HNSW 的 `semantic_search_with_embedding` 旧实现只处理 material 记录、
   跳过 memory 记录 → 异步写入的学习素材在素材检索里查不到。
   现已用统一的 `memory_to_material()` 还原并参与 owner 过滤；
   `get_material` / `count_materials` / `list_materials` 同步支持 memory 记录。
2. **F-VS7（复现测试暴露的底层 bug）**：`VectorStore::insert` 解析 owner 时只认小写 `"agent"`，
   而写入侧用 `format!("{:?}")` 产生 `"Agent"` → **Agent 作用域的向量被落成 User**，
   导致按 Agent 过滤永远查不到。现改为 `OwnerType::parse`（大小写不敏感）。

**复现证据**：修复前 `test_memory_record_visible_to_material_apis` 断言
`hits.len() == 1` 实际得到 `0`；修复后通过。

**仍存在**：
1. 同步路径仍用 material API 写入、异步用 memory 写入（检索已收敛，写入仍分叉）。

### R10 ✅ 已定口径并实现：素材统计范围

**用户定义**：`count_materials` 统计的是**技术类学习素材、经验类数据集**；
常规泛用文本、文章、诗歌等不计入。

**实现**：`stores/vector/types.rs` 新增
`COUNTED_MATERIAL_TYPES = ["learning", "technical_learning", "experience_dataset"]`
与 `is_countable_material_type()`（大小写不敏感）。
两个向量库的 `count_materials` / `list_materials` 都按白名单过滤；
`event`、`file_chunk`（文章/诗歌以文件进入时属此类）不再计入。
后续若在提取阶段细分"技术/经验"两类，只需把新类型加入白名单。

### R2 🔴 自动合并（相似度 > 0.90）无法回滚

合并修改的是**既有事件**（追加联系人、摘要），没有快照。写入失败时补偿只能删"本次新建"的行，
撤销不了已合并的内容 → 可能出现"失败的这次写入污染了历史事件"。
需要：合并前快照，或把合并改为"先记录合并意图、提交后生效"。

### R3 🟠 字典库可能出现"指向已回滚事件"的历史记录

补偿会删关系库 `event_index` 行，但字典库 `contact_event_history` 按设计**永不删除**。
若一个对话里事件 A 已写字典历史、事件 B 失败触发补偿，A 的索引被删而历史还在。
这是"字典是源文件"的必然结果，重放会重建索引；但需要心跳/重放任务把这类悬空记录对上。

### R4 🟠 跨库补偿只覆盖"本次新建"，不覆盖"更新"类操作

`write_contacts` 的 `patch_contact`、`upsert_contact_index`、`update_event_after_merge`、
待办 deadline 更新都是**原地修改**，失败后无法恢复旧值。
当前依赖"这些操作幂等/可重放"，但没有版本号或旧值快照。

### R5 🟠 提取任务的"失败"仍可能重复计数

`process_task` 失败后：意图置 `aborted`、补偿删除、对话置 `failed`。
但队列 `recover_pending_tasks` 只恢复 `pending_tasks` 树里的任务；
若任务在失败时已被 `remove_from_sled`，该对话不会自动重试。

**现状（2026-10-28 更新）**：已补**单条 + 批量**重放入口
（`POST /api/memory/replay/:conversation_id`、`POST /api/memory/replay_failed?limit=N`），
可从字典库原文重新入队，并按 `failed` 状态批量补跑。

### R6 🟠 时间语义两套：事件时间 vs 提醒日界

`parse_event_time` 以 **UTC** 解析"今天/明天"；待办提醒用 UTC 23:59:59 作日界。
UTC+8 下两者都偏移 8 小时。已在 `FIX_LIST` 记为 F-T6 / D-M8 类问题，**未修**。
修之前要先确定：记忆系统的"日"以服务器本地时区还是配置时区为准（建议配置化）。

### R7 🟡 `data_dispatcher` 字段仍在，`dispatcher.rs` 已死

`ThreeLibraryMemoryManager` 保留该字段只为兼容 5 处构造调用（server/init/tests/examples）。
删除它需要同步改这 5 处；`dispatcher.rs` 400+ 行可整体删除。
**删除前确认没有外部调用方**（当前 grep 结果为 0）。

### R8 🟡 配置项已加但未全部被读取

已接线：`[heartbeat]`、`[todo]`、`[extraction].timeout_secs/intent_grace_secs`、`[event]`、`[agent]`。
仍需确认：`[storage].question_pool_dir`（已接线）、`[llm.*].timeout`（GGUF 内部 180s 仍硬编码，F-L1）、
`[embedder].model_path` 与 `FASTEMBED_CACHE_DIR` 的优先级。

### R9 🟡 测试覆盖偏"单元"，缺端到端回滚验证

现有 57 个库测试覆盖：原子写入、意图状态机、联系人字段保留、虚拟角色跳过、素材质量过滤、配置解析。
**缺**：一次真实"写入中途失败 → 补偿删除 → 重放成功"的集成测试（需要可注入失败的 VectorStore/RelationalStore mock）。

---

## 三、待修清单（按下一步执行顺序）

### P1 — 收尾阶段 4（统一写入层）

| 序号 | 事项 | 依赖 | 风险 |
|------|------|------|------|
| 1 | 事件写入迁入 `MemoryWriter::write_events()`（0.75/0.90 阈值、合并检查、写序、补偿登记） | 无 | 中（体量大，需逐段搬迁 + 对比行为） |
| 2 | 解决 R1：向量写入统一（适配器或检索侧改读 metadata） | 需先定方案 | 🟡 检索侧已修（HNSW 兼容 memory 记录）；待补 `get_material`/计数/列表 |
| 3 | 删除 `dispatcher.rs` + `data_dispatcher` 字段（R7） | 1、2 完成 | 低（机械改动） |

### P1 — 可靠性补强

| 序号 | 事项 | 说明 |
|------|------|------|
| 4 | 重放工具/接口（R5） | ✅ 单条 + 批量均已实现 |
| 5 | 合并快照或合并意图（R2） | 避免失败写入污染既有事件 |
| 6 | 端到端回滚测试（R9） | 注入失败，断言派生数据被清理、字典保留 |
| 7 | 文件向量孤儿清理（F-F8） | 按 `source_file_id` 清向量；用户已定"物理删除由心跳处理" |

### P2 — 一致性/体验

| 序号 | 事项 |
|------|------|
| 8 | 时区配置化（R6 / F-T6） |
| 9 | 虚拟实体别名替换加事务（F-V1） |
| 10 | 归档数据"降一级"检索规则落地（阶段 5） |
| 11 | 字典 tombstone（阶段 5，`deleted_at` 标记，不物理删） |

---

## 四、变更安全守则（每次改写入层都要过一遍）

1. 字典库只追加，不删除、不整条覆盖（用 `ContactPatch` 这类增量补丁）。
2. 写序：关系库 → 向量库 → 字典库；字典最后写。
3. 新建资源必须登记到 `WriteOutcome`，失败时 `compensate_write` 才能回滚。
4. owner 只从 `core::owner` 取，禁止字面量。
5. 阈值只从配置/`MemoryWriterConfig` 取，禁止散落常量。
6. 每次改动跑 `cargo check --lib --bin shouxin-server` + `cargo test --lib`，基线 57 passed。
