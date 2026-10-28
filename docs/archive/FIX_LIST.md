# 守心 · 最终修复清单（FIX_LIST）

> 生成日期：2026-10-28
> 依据：`MODULE_CHECK_LOG.md`（模块 1-14 全部检查完成）
> 待修总数：**110 项**（P0 14 / P1 41 / P2 55）
> 说明：本轮只检查、只记录；除"已完成"区列出的项外，**未修改任何代码**。

---

## 一、已完成（不计入 110 项）

| 编号 | 内容 | 状态 |
|------|------|------|
| Fix #1 | 提取超时 600s → `[extraction].timeout_secs`（配置化） | ✅ 已改、编译通过 |
| D-M1 | 入队失败/未配置队列 → 标记 `failed` 并记录原因 | ✅ 已改、编译通过 |
| D-M2 | 决策不实现自动重试；修正 `PROJECT_INTRO` / `ASYNC_EXTRACT_DESIGN` | ✅ 文档已改 |
| D-M3 | 未配置队列不再静默跳过 | ✅ 随 D-M1 完成 |
| D-M9 | 入队日志字节数 → 字符数 | ✅ 已改 |
| — | `add` 强事务：原文 + 提取任务同一 sled 事务（新增 `save_conversation_with_task`） | ✅ 已改，含单测 |
| F-SS1 | 智能搜索 `history` 按关键词过滤 | ✅ 已改 |
| F-E6/F-E7 | 事件合并/待确认插入失败 → 回退创建新事件，不丢源事件 | ✅ 已改 |
| F-X4/F-X5 | 异步学习素材补齐 `quality>=0.8` 过滤 + 0.98 去重 | ✅ 已改 |
| F-M1 | 新增 `ContactPatch` + `patch_contact`：只合并显式字段，不再清空 aliases/background/notes/tags/related/weight | ✅ 已改，编译通过 |
| F-M2 | 异步路径写入字典库 `contact_event_history`（新建事件 + 合并事件均写） | ✅ 已改 |
| F-M3 | 补丁携带真实 `owner_type`/`owner_id`，Agent 联系人不再被写成 User | ✅ 已改 |
| F-T1 | `"primary_agent"` 硬编码全部收敛到 `core::owner::primary_agent_id()`（启动时由 `[agent].primary_agent_id` 注入） | ✅ 已改 |
| F-T2 | `resolve_owner` 支持 Agent 归属；未命中时回退默认作用域（agent） | ✅ 已改 |
| F-F3 | 文件 get/get_text/delete 增加 owner 校验（新增 `FileOwnerQuery`） | ✅ 已改 |
| F-H0 | `[heartbeat].enabled` 真正生效（`spawn` 前判断） | ✅ 已改 |
| F-H1 | 自适应间隔实现：`next_interval_secs()` 按"距上次记忆写入"在 min/base/max 间切换，并夹紧区间 | ✅ 已改，含 3 个单测 |
| F-VS5 | 删除"过滤为空则无过滤重试"；`memory_to_material` 从 metadata 读取真实 owner/material_type | ✅ 已改 |
| F-H3 | 心跳维护/LLM 超时配置化（`maintenance_timeout_secs`/`llm_timeout_secs`） | ✅ 已改 |
| F-H2 | `storage.question_pool_dir` 真正生效（原写死 `.heartbeat_data`） | ✅ 已改 |
| F-T3 | 待办 30/50 阈值配置化（新增 `[todo]`：max_pending_before_archive / archive_keep_count / reminder_threshold / archive_after_days） | ✅ 已改 |
| F-T4 | `update_todo_deadline` 同步刷新 `last_updated_at` | ✅ 已改 |
| F-F1 | `insert_file` 返回是否插入成功；竞态时清理临时文件并返回既有记录 | ✅ 已改 |
| F-F2 | 删除 pending/failed 文件时清理临时文件 | ✅ 已改 |
| C1 | `tests/common.rs` 硬编码 `D:\OpenClaw-Workspace\...` → `FASTEMBED_CACHE_DIR` 或项目根 `.fastembed_cache` | ✅ 已改 |
| — | `time_parser` 三个真实缺陷：按捕获组数分派导致"6月15日"→06:15、`4 if` 分支不可达、时段词 HashMap 遍历不确定；并修正"下周三"语义 | ✅ 已改，原 2 个失败测试转绿 |
| — | 事件重要性/情感分类收敛为 `utils::event_meta`（同步/异步唯一来源） | ✅ 已改 |
| F-M5 | **统一写入层起步**：新增 `MemoryWriter::write_contacts()`，同步 `add()` 与异步 worker 共用；同步路径原本硬编码 `aliases="[]"` 等会清空关系库字段的问题一并修掉 | ✅ 已改，含 3 个单测 |
| F-T3 | `[todo]` 配置段补进 `config.toml` / `config.toml.example`（原先只有代码默认值，用户无从配置） | ✅ 已补 |
| — | 同步 `add()` 中的死代码 `dispatch_input` 删除；确认 `DataDispatcher::dispatch()` 全库无调用方（第三套死实现），`data_dispatcher` 字段标注为待删除 | ✅ 已清理/已记录 |
| — | 写序调整：事件路径改为「关系库 → 向量库 → 字典库」，字典（不可回滚）最后写，为跨库回滚做准备 | ✅ 已改 |
| — | 新增 `config.toml` 真实解析回归测试（防新增配置段 key 写错） | ✅ 已加 |
| F-M4（第一步） | 跨库提交意图状态机：新增 `commit_intents` 树、`WriteIntent{prepared/committed/aborted}`、`record_intent_resources`，每次提取写入先落意图再置终态 | ✅ 已改，含 2 个单测 |
| F-M4（第一步） | 启动恢复：陈旧 `prepared` 意图 → `aborted`，对应对话置 `failed` 并标注 needs replay；阈值 `[extraction].intent_grace_secs`（默认 300s） | ✅ 已改 |
| **F-M4（完成）** | **失败补偿删除**：新增 `RelationalStore::delete_event_by_id` / `delete_pending_event_merge_by_id`；进程内失败按 `WriteTracker` 回滚事件索引/待确认合并/向量记忆；崩溃残留按意图登记的 ID 在启动时回滚 | ✅ 已改 |
| — | 向量写入失败不再静默 `warn`：事件/素材向量写失败即整个写入失败并触发补偿（强事务要求） | ✅ 已改 |
| F-X4/F-X5（收口） | 学习素材写入迁入 `MemoryWriter::write_learnings()`（质量过滤 + 0.98 去重 + 向量入库 + 失败即错误），阈值走 `MemoryWriterConfig` | ✅ 已改，含 2 个单测 |
| — | 补偿资源记录统一为 `memory::WriteOutcome`（写入层登记、worker 回滚），删除 worker 内重复的 tracker 定义 | ✅ 已改 |
| — | 新增失败任务重放入口 `POST /api/memory/replay/:conversation_id`（从字典原文重新入队，状态回 `pending`，不删任何字典数据） | ✅ 已改 |
| — | 新建 `RISKS_AND_PENDING.md`：登记 9 条风险（双向量 API 分叉、合并不可回滚、字典悬空历史、原地更新不可回滚、时区语义等）与待修优先级 | ✅ 已建 |
| **F-VS6** | HNSW 素材检索旧实现只处理 material 记录、跳过 memory 记录 → 异步路径写入 HNSW 的学习素材在素材检索中完全查不到；现按 `memory.metadata` 还原 `LearningMaterial` 并参与 owner 过滤；详情/计数/列表同步兼容 | ✅ 已改，含复现测试 |
| **F-VS7** | `VectorStore::insert`（HNSW）解析 owner 时只认小写 `"agent"`，写入侧却是 `"Agent"` → **Agent 作用域向量被落成 User**，按 Agent 过滤永远查不到；改为 `OwnerType::parse`（大小写不敏感） | ✅ 已改，含回归测试 |
| **F-VS8 / R10** | 素材统计口径落地：`count_materials`/`list_materials` 只统计"技术类学习素材/经验数据集"（白名单 `learning`/`technical_learning`/`experience_dataset`），事件与文件分块（文章、诗歌等）不计入；两个向量库同步实现 | ✅ 已改，含 2 个测试 |
| **F-V1** | 虚拟角色别名替换改为单事务（`replace_aliases`：DELETE + 批量 INSERT 在同一 SQLite 事务），失败回滚并返回错误，不再 `.ok()` 吞掉导致别名丢失 | ✅ 已改 |
| **R5（完成）** | 批量重放：`POST /api/memory/replay_failed?limit=N`（默认 20 / 上限 200），按 `failed` 状态扫描对话并逐条重投；新增 `list_failed_conversation_ids` | ✅ 已改，含单测 |

> 当前验证基线：`cargo check --lib --bin shouxin-server` exit 0（26 warnings，全部为既有 dead_code/unused）；`cargo test --lib` **62 passed / 0 failed**；`test_file_upload` 测试目标可编译（运行需嵌入模型缓存）。

---

## 二、P0（14 项：数据正确性 / 隔离 / 安全）

> **状态：14/14 已修复**（见第一节"已完成"表）。保留原始问题描述作为验收依据。

| # | 编号 | 模块 | 问题 | 修复方向 |
|---|------|------|------|----------|
| 1 | F-M1 | 记忆 | 异步写回用 `ContactRecord` 覆盖字典库 `ContactTotal`，aliases/background/notes/tags/related/weight 被清空 | 扩展 trait 存 `ContactTotal`，或复用 `DataDispatcher` |
| 2 | F-M2 | 记忆 | 异步路径不写 `contact_event_history` | 补 `save_contact_event`，或复用 dispatcher |
| 3 | F-M3 | 记忆 | `ContactRecord` 无 `owner_type`，trait 硬编码 `User`，Agent 数据被写成 User | 传 owner_type，扩展 trait |
| 4 | F-T1 | 待办 | Agent owner 硬编码 `"primary_agent"`（`contact.rs:61`、`routes.rs:1171/455`、`internal_api.rs:46`），与配置 `primary_agent_default` 冲突 | 统一走 `Scope::to_owner_params` + 注入 `primary_agent_id` |
| 5 | F-T2 | 待办 | `resolve_owner` 只推断 User；未传 scope 时 Agent 待办回退 `(User,"default_user")`，查不到 | 支持按 `get_todo_owner` 返回的 owner_type 还原 |
| 6 | F-F3 | 文件 | `get_file_metadata` / `get_file_text` / `delete_file` 无 owner 校验，跨 owner 越权 | 加 scope/user_id 参数并校验 |
| 7 | F-H0 | 心跳 | `settings.heartbeat.enabled` 从未被读取，关不掉心跳 | 启动前判断 enabled |
| 8 | F-H1 | 心跳 | `min/max/adaptive/idle_threshold` 传入后从未使用，自适应未实现 | 实现自适应调度或删除死字段 |
| 9 | F-E6 | 事件合并 | 自动合并失败只 warn 后 `continue`，源事件既不合并也不创建 → 数据丢失 | 失败时回退"创建新事件" |
| 10 | F-E7 | 事件合并 | 待确认记录插入失败只 warn 后 `continue`，源事件丢失 | 同上 |
| 11 | F-SS1 | 智能搜索 | `history` 过滤条件是 `true`，完全不做关键词匹配 | 实现 history 关键词过滤 |
| 12 | F-X4 | 提取 | 异步路径无 0.98 学习素材去重（仅同步路径有） | 异步补齐 0.98 去重 |
| 13 | F-X5 | 提取 | 异步路径无 `quality_score >= 0.8` 过滤 | 异步补齐质量阈值 |
| 14 | F-VS5 | 向量 | owner 过滤为空时回退"无 owner"搜索，可能跨 owner 返回数据 | 回退前确认是否需要跨 owner |

---

## 三、P1（41 项：功能缺口 / 一致性 / 可靠性）

| # | 编号 | 模块 | 问题 | 修复方向 |
|---|------|------|------|----------|
| 1 | F-M4 | 记忆 | 异步写三库无回滚/事务，失败留部分写入 | 引入事务或补偿 |
| 2 | F-M5 | 记忆 | 同步 `DataDispatcher` 与异步 `write_extraction_results` 双写路径重复 | 抽统一 MemoryWriter |
| 3 | D-M5 | 记忆 | worker 用 `spawn_blocking` + 新建 runtime + `.expect()` | 直接 async 执行，去掉 expect |
| 4 | F-T3 | 待办 | 30/50 阈值与 keep_count=50 两处硬编码 | 收敛到配置 |
| 5 | F-T4 | 待办 | `update_todo_deadline` 不更新 `last_updated_at` | 同步更新 |
| 6 | F-T5 | 待办 | 应用层分钟精度去重 vs 唯一索引精确 deadline | 统一语义 |
| 7 | F-T6 | 待办 | 提醒/去重用 UTC 日界，UTC+8 下日界为本地 08:00 | 改本地时区或配置 |
| 8 | F-H2 | 心跳 | `storage.question_pool_dir` 未使用，硬编码 `.heartbeat_data` | 读配置 |
| 9 | F-H3 | 心跳 | 维护 30s / LLM 120s / analyzer 90s / 歧义 120s 全硬编码 | 配置化 |
| 10 | F-H4 | 心跳 | 内存 4096MB、跳过 3、归档批量 1000、画像 2 次等硬编码 | 配置化 |
| 11 | F-F1 | 文件 | 并发重复上传竞态：`INSERT OR IGNORE` 不检查 rows_affected | 检查影响行数并清理临时文件 |
| 12 | F-F2 | 文件 | 删除不清理 `pending/failed` 临时文件 | 删除时读 original_path 清理 |
| 13 | F-F4 | 文件 | `tokio::spawn` 后台任务无重启恢复，pending 永远 pending | 启动扫描 pending 重投 |
| 14 | F-F5 | 文件 | 20MB 检查在完整 body 缓冲后，内存 DoS 风险 | 流式限制/提前拒绝 |
| 15 | F-F6 | 文件 | `owner_type` 存小写 `user/agent`，其他模块存 `User/Agent` | 统一枚举序列化 |
| 16 | F-F8 | 文件 | 删除文件不清理 `source_file_id` 对应向量，孤儿向量 | 按 metadata 清理 |
| 17 | F-F11 | 文件 | `.doc/.xls` 不支持却被当 `txt` 接受 | 拒绝或真正支持 |
| 18 | F-V1 | 虚拟实体 | update 别名替换无事务、错误被 `.ok()` 吞，可能全丢 | 事务 + 错误传播 |
| 19 | F-V2 | 虚拟实体 | add 别名失败仍返回成功 | 事务/返回错误 |
| 20 | F-V3 | 虚拟实体 | delete 先删别名后删联系人，非原子 | 事务 |
| 21 | F-V4 | 虚拟实体 | `ContactIndex.aliases` JSON 不随别名/改名更新 | 同步维护 |
| 22 | F-V5 | 虚拟实体 | 重名创建返回"内部错误"而非 409 | 预检查/友好错误 |
| 23 | F-V7 | 虚拟实体 | 孤立别名导致匹配提前返回 None | 未命中联系人时继续查名 |
| 24 | F-V9 | 虚拟实体 | 无 owner/scope 校验 | 加 owner 约束 |
| 25 | F-E1 | 事件合并 | 0.75/0.90 阈值硬编码 | 配置化 |
| 26 | F-E3 | 事件合并 | confirm 仅按 merge_id，无 owner 校验 | 加 owner 校验 |
| 27 | F-E4 | 事件合并 | 确认合并多步写入无事务 | 事务 |
| 28 | F-VS1 | 向量 | conversation_id 过滤在 limit 之后 | 先过滤再截断 |
| 29 | F-VS2 | 向量 | `GET /api/vector/:id` 无 owner 校验 | 加校验 |
| 30 | F-SS2 | 智能搜索 | 各来源错误被吞，部分失败仍 200 | 返回部分失败信息 |
| 31 | F-SS3 | 智能搜索 | todos 全量取出内存过滤 O(n) | SQL 层过滤 |
| 32 | F-I1 | 内部 API | `default_owner_id` 硬编码 primary_agent | 用配置 |
| 33 | F-I2 | 内部 API | 内部事件不写字典库 | 补字典写入 |
| 34 | F-A1 | 管理 API | switch 失败返回 200 + success:false | 改 4xx |
| 35 | F-A2 | 管理 API | 无速率限制 | 加限流 |
| 36 | F-A3 | 管理 API | token 为空仅 warn，服务照常启动 | 启动失败或显式禁用 |
| 37 | F-L1 | LLM | GGUF 内部 180s 与配置 600s 不一致 | 读配置 |
| 38 | F-L2 | LLM | Ollama/API timeout 未完全走配置 | 统一读配置 |
| 39 | F-L3 | LLM | 失败阈值 3、健康检查 30s 硬编码 | 配置化 |
| 40 | F-S1 | 三库 | 异步/同步写路径不一致（同 F-M1/M2/F-X4/X5） | 随 F-M5 统一 |
| 41 | F-S3 | 三库 | 三库写入无跨库事务 | 随 F-M4 |

---

## 四、P2（55 项：硬编码 / 配置 / 文档 / 优化）

| # | 编号 | 模块 | 问题（摘要） |
|---|------|------|--------------|
| 1 | D-M4 | 记忆 | `MAX_ACTIVE_CONTACTS=50` 双处硬编码 |
| 2 | D-M6 | 记忆 | `BackgroundTaskQueue::new()` 产生静默丢任务实例 |
| 3 | D-M7 | 记忆 | `.cargo/config.toml` 写死旧路径 `d:\OpenClaw-Workspace\...` |
| 4 | D-M8 | 记忆 | `push_with_persist` 同步 flush，可能拉高 add 延迟 |
| 5 | F-M6 | 记忆 | `completed` 状态写回错误被 `let _ =` 忽略 |
| 6 | F-M7 | 记忆 | `activity_score` 转换时固定 0.0 |
| 7 | F-T7 | 待办 | `schema.sql` 缺 v0.5.1 列，靠 ALTER 补 |
| 8 | F-T8 | 待办 | `TodoItem` 字段在 TECHNICAL/EXTENSION 文档写反 |
| 9 | D-H1 | 心跳 | 维护阶段内容与文档不符 |
| 10 | D-H2 | 心跳 | QuestionPool 实为 sled，文档写 SQLite+文件 |
| 11 | D-H3 | 心跳 | 文档称配置未使用，实际已读取 |
| 12 | D-H4 | 心跳 | HeartbeatStatus 字段与文档不一致 |
| 13 | D-H5 | 心跳 | HeartbeatReport 字段类型文档不一致 |
| 14 | D-H6 | 心跳 | 默认间隔三套值不一致 |
| 15 | F-H5 | 心跳 | `run_heartbeat` 无调用方；LLM 失败标记误导 |
| 16 | F-H6 | 心跳 | `get_pending_questions(usize::MAX)` 全量加载 |
| 17 | F-H7 | 心跳 | `next_heartbeat_in_secs` 未扣已过时间 |
| 18 | F-H8 | 心跳 | 联系人归档批量 1000 写死 |
| 19 | D-F1 | 文件 | `filename` 文档可选实际必填 |
| 20 | D-F2 | 文件 | 上传响应字段与文档不一致 |
| 21 | D-F3 | 文件 | `PROJECT_INTRO` 写不存在的 `/api/files/list` |
| 22 | D-F4 | 文件 | 临时目录文档 `./temp_uploads` vs 实际 `<data_dir>/temp_uploads` |
| 23 | D-F5 | 文件 | 文档称支持 doc/xls，实际只支持 docx/xlsx |
| 24 | D-F6 | 文件 | `owner_type` 小写/大小写口径不一致 |
| 25 | F-F7 | 文件 | `user_id` 默认 `primary_agent` 硬编码 |
| 26 | F-F9 | 文件 | 失败路径返回 `Ok(())`，日志误导 |
| 27 | F-F10 | 文件 | 元数据返回 `original_path` 绝对路径 |
| 28 | F-F12 | 文件 | 关联插入失败只 `eprintln` |
| 29 | F-F13 | 文件 | 逐 chunk 串行 embed，大文件慢 |
| 30 | D-V1 | 虚拟实体 | check 未命中返回 null 非错误 |
| 31 | D-V2 | 虚拟实体 | 响应结构与文档不一致 |
| 32 | D-V3 | 虚拟实体 | `ContactIndex.aliases` 未承载别名 |
| 33 | F-V6 | 虚拟实体 | 别名表无唯一约束 |
| 34 | F-V8 | 虚拟实体 | get/check 未命中 200 null |
| 35 | F-V10 | 虚拟实体 | list 无分页 |
| 36 | F-V11 | 虚拟实体 | 同名用户角色导致内置别名不补齐 |
| 37 | D-E1 | 事件合并 | 文档 top 3 vs 实际 top 5 |
| 38 | F-E2 | 事件合并 | `event_similarity_cache` 死表 |
| 39 | F-E5 | 事件合并 | 非法 owner_type 静默默认 User |
| 40 | D-VS1 | 向量 | 文档 usearch vs 实际暴力余弦 |
| 41 | F-VS3 | 向量 | 768 维默认构造硬编码 |
| 42 | F-VS4 | 向量 | 两个路由同 handler，维护歧义 |
| 43 | D-SS1 | 智能搜索 | 文档称 history 内存过滤，实际未过滤 |
| 44 | D-I1 | 内部 API | 只写 ContactIndex，未持久化 ContactTotal |
| 45 | D-I2 | 内部 API | 配置 token 对内部 API 不生效，只认环境变量 |
| 46 | F-I3 | 内部 API | importance/importance_score 硬编码 |
| 47 | F-I4 | 内部 API | register 无输入校验 |
| 48 | D-A1 | 管理 API | 文档 X-Internal-Token vs 实际 Bearer |
| 49 | D-A2 | 管理 API | 状态缺 health_check_interval_secs |
| 50 | F-L4 | LLM | 重试 2 次/退避公式硬编码 |
| 51 | D-S1 | 三库 | 文档 vector_dir "vector" vs 实际 "vector_store" |
| 52 | F-S2 | 三库 | `store.rs` 两处 max_connections(20) 与 64 不一致 |
| 53 | F-X1 | 提取 | 截断 1024 字符硬编码 |
| 54 | F-X2 | 提取 | 乱码阈值 0.05/0.30 硬编码 |
| 55 | F-X3 | 提取 | 质量阈值 0.8 两处硬编码 |

---

## 五、三大根因

1. **双写路径**（F-M5 / F-S1）：v0.5.1 异步化时 worker 重写了一套写库逻辑，导致 F-M1/F-M2/F-M3/F-M4/F-X4/F-X5 这一整串问题。**优先抽统一 MemoryWriter，是收益最高的一步。**
2. **owner/scope 硬编码**（F-T1 / F-M3 / F-F3 / F-F6 / F-VS5 / F-I1）：
   - `"primary_agent"` 与 `"primary_agent_default"` 两套值并存；
   - `ContactRecord` / 文件表用不同 owner 表示法；
   - 多个读/删接口不做 owner 校验。
3. **配置未接线**（F-H0 / F-H1 / F-H2 / F-L1 / F-L2 / F-L3 / F-T3 / F-H3 / F-H4 / F-X1~X3）：
   - 配置项存在但不读（enabled、question_pool_dir、min/max/adaptive）；
   - 代码里另有硬编码值（180s、30s、4096MB、0.8、1024）。

---

## 六、建议修复顺序

| 阶段 | 内容 | 涉及 |
|------|------|------|
| **P1 阶段：统一写入** | 抽 `MemoryWriter`（或扩展 trait），让同步/异步共用；补齐事件历史、字段保留、0.98 去重、质量过滤、回滚 | F-M1~F-M5、F-X4/F-X5、F-S1/F-S3 |
| **P2 阶段：owner 统一** | 统一 `primary_agent_id` 来源；`Scope` → owner 参数唯一入口；读/删接口补 owner 校验；`owner_type` 序列化统一 | F-T1/F-T2、F-M3、F-F3/F-F6、F-VS5、F-I1 |
| **P3 阶段：可靠性** | 心跳 enabled/自适应接线；事件合并失败回退；虚拟实体别名事务；文件 pending 恢复 + 临时文件/向量清理；并发去重竞态 | F-H0/F-H1、F-E6/F-E7、F-V1/F-V2、F-F1/F-F2/F-F4/F-F8 |
| **P4 阶段：配置化** | 超时、阈值、批量、质量分、截断长度全部收敛到 `[extraction]` / `[heartbeat]` / `[file]` / `[llm]` / `[todo]` | 全部 F-* 硬编码项 + D-M4/F-T3/F-H3/F-H4/F-L1~L4/F-X1~X3 |
| **P5 阶段：文档治理** | 以 `TECHNICAL_DOCUMENTATION + API_REFERENCE + FEATURE_MODULES_CN` 为权威，修正所有 D-*；合并重复文档；补 CHANGELOG | 全部 D-* |
| **P6 阶段：优化** | 串行 embed 批量化、列表分页、智能搜索 SQL 化、死表/死代码清理、`cargo check` 警告清理 | F-F13、F-V10、F-SS3、F-E2、F-H5、F-H6、F-VS4 |

---

## 七、备注

- 本清单不含"已完成"的 5 项（Fix #1、D-M1/M2/M3/M9）。
- 检查过程中 `cargo check --lib --bin shouxin-server` 已通过（rustc 1.99.1，32 个原有 warning）。
- 建议每完成一个阶段跑一次 `cargo check` 与相关集成测试，避免一次性大改。

---

## 四、2026-10-28 启动阻塞与真实数据跑通（本轮新增）

| 编号 | 内容 | 状态 |
|------|------|------|
| F-EMB1 | 配置把**缓存目录**当**模型名**传给 FastEmbed（`model_path` vs `model` 混用）→ 嵌入模型永远加载失败 | ✅ 已改，配置拆成 `model` / `model_path` |
| F-EMB2 | 嵌入模型硬依赖：缺模型即退出启动 | ✅ 已改：`[embedder] backend = fastembed\|hash` + `fallback_to_hash`，新增离线哈希嵌入器（语义可用、非零向量） |
| F-EMB3 | 本机无 ONNX 模型且 huggingface.co 直连受限 | ✅ 已改：`scripts/fetch_embedding_model.sh` 走 hf-mirror 下载到工作区缓存（HF 布局 + 代码期望布局各一份） |
| F-BLD1 | `--features gguf` 构建失败（cmake 缺失 / VS 生成器触发 reg.exe / llama.cpp UTF-8 触发 C2001） | ✅ 已改：`scripts/build_gguf.sh` 一键解决（Ninja + 显式 cl.exe + `/utf-8` + 编码补丁） |
| F-BLD2 | 构建脚本内部再拉起 cmake/ninja 时因 **PATH 为 POSIX 形态**导致静默退出 1 | ✅ 已改：`CMAKE` / `CMAKE_MAKE_PROGRAM` 改用 Windows 绝对路径 |
| F-RUN1 | 运行期缺 `cublas64_12.dll`（CUDA DLL 不在 PATH） | ✅ 已解决：启动时把 CUDA `bin` 加入 PATH（GPU 卸载 29/29 层成功） |
| F-TODO2 | 待办"模糊匹配"名不副实（只做 LIKE 连续子串）→ "写报告"匹配不到"写项目报告"，更新待办找不到目标 | ✅ 已改：改为子序列模糊匹配（含单测） |
| F-TEST1 | 集成测试长期编译不过（`activity_score` 缺字段、`archive_stale_todos` 少参数） | ✅ 已改：测试同步到当前 API |
| — | 真实数据端到端：llama.cpp(GGUF) 推理 10.67s → 解析 2 人 5 事件 → 联系人/待办入库 → 检索回读，**全链路跑通** | ✅ 已验证 |

> 本轮验证基线：`cargo test --tests` **156 passed / 0 failed**（库 65 + 集成 91）。

---

## 五、2026-10-28 22:00 CPU 推理改造与真实数据验证

| 编号 | 内容 | 状态 |
|------|------|------|
| F-CPU1 | LLM 推理改为**纯 CPU**：`[llm.gguf].use_gpu=false`；心跳专用 LLM 客户端原先硬编码 99 层 GPU，一并关闭 | ✅ |
| F-CPU2 | 剥离 CUDA 依赖：`gguf` 特性改为纯 CPU 构建，新增 `gguf-cuda` 可选；二进制不再依赖 `cublas64_12.dll`，构建时间 15min→1min | ✅ |
| F-CPU3 | 构建脚本 `RUSTFLAGS=-C target-cpu=native` 触发 `GGML_NATIVE=ON`；参数调优 threads=16 / batch=512 / ctx=4096 / max_tokens=1024 / 提取超时=1800 | ✅ 构建成功 |
| F-CPU4 | 构建脚本加固：`-utf-8`（避开 Git Bash 路径转换）、`MSYS2_ARG_CONV_EXCL=*`、Windows 绝对路径指定 cmake/ninja | ✅ |
| F-PERF1 | **CPU 推理性能未达标**：单次 131.6s/187 字符（≈0.9–1.4 tok/s），与同硬件预期（20–40 tok/s）差 20–40 倍；native 构建未改善。已登记 4 项待验证疑点 | ⚠️ 待定位 |
| F-DL1 | 待办 deadline 改为**优先本地 time_parser 解析**（模型给的过期日期一律丢弃），只认精确时间表达（排除"下季度"这类范围词） | ✅ 含 3 单测 |
| F-TP1 | `parse_event_time` 的"下周X"多算一周（"下周一"→11-09 应为 11-02），公式修正 `7-since+offset` | ✅ |
| F-API1 | 新增 `GET /api/events/pending_merges`、`POST /api/events/pending_merges/{id}/resolve`（merge/create/dismiss） | ✅ |
| F-API2 | 修 `pending_merges` 列表 `premature end of input`：SQLite `CURRENT_TIMESTAMP` 非 RFC3339；新增 `parse_sqlite_datetime()`，并修掉 `owner_type` 硬编码 | ✅ 实测 count=11、resolve 正常 |
| — | 真实数据验证：短对话提取出联系人"小王"、语义检索命中历史事件；长对话（复杂场景）在 CPU 下超时后**自动降级正则**（3 人/5 事件/8 素材），未丢数据 | ✅ 降级链路健康 |

> 验证基线：`cargo test --tests` **160 passed / 0 failed**（库 69 + 集成 91）。
