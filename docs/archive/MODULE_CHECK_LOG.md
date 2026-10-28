# 守心 · 模块检查日志（累积）

> 开始日期：2026-10-27
> 原则：**先设计核对 → 再功能核对 → 只记录不修**（用户指示）
> 顺序：`FEATURE_MODULES_CN.md` 的 14 个功能模块
> 进度：**模块 1-14 全部检查完成** ✅
> 最终产出：`FIX_LIST.md`（110 项 + 修复状态）；本文件保留逐模块原始证据
> 修复状态：P0 14/14 已修（详见 `FIX_LIST.md` 第一节），其余按 P1/P2 顺序推进

---

## 模块 1：记忆管理 ✅

### 设计层问题（D-M）

| 编号 | 严重度 | 问题 | 状态 |
|------|--------|------|------|
| D-M1 | 🔴 | 入队失败被吞，仍返回成功 | ✅ 已修：标记 failed + 记录原因 |
| D-M2 | 🔴 | 无自动重试（`retry_count` 从不递增，失败即删 sled） | ✅ 已决策不实现；`PROJECT_INTRO` / `ASYNC_EXTRACT_DESIGN` 已修正 |
| D-M3 | 🟠 | 未配置队列时静默跳过提取 | ✅ 已修：标记 failed |
| D-M4 | 🟠 | `MAX_ACTIVE_CONTACTS=50` 在 `background_queue.rs:17` 与 `dispatcher.rs:36` 双处硬编码 | ⏸ 待修 |
| D-M5 | 🟠 | worker 用 `spawn_blocking` + 新建 runtime + `.expect()` | ⏸ 待修 |
| D-M6 | 🟡 | `BackgroundTaskQueue::new()` 产生非持久化、直接丢任务的实例 | ⏸ 待修 |
| D-M7 | 🟡 | `.cargo/config.toml` 写死旧路径 `d:\OpenClaw-Workspace\...` | ⏸ 待修 |
| D-M8 | 🟡 | `push_with_persist` 在请求路径同步 `insert + flush`，可能拉高 add 延迟 | ⏸ 待评估 |
| D-M9 | 🟢 | 入队日志用字节数当字符数 | ✅ 已顺手修为 `chars().count()` |

### 功能层问题（F-M）

| 编号 | 严重度 | 问题 | 证据 | 状态 |
|------|--------|------|------|------|
| F-M1 | 🔴 | 异步路径用 `ContactRecord` 写字典库，转 `ContactTotal` 时清空 aliases / background / notes / tags / related / weight | `background_queue.rs:376-391`、`store.rs:743-774` | ⏸ 待修 |
| F-M2 | 🔴 | 异步路径不写 `contact_event_history`，字典库缺"联系人事件历史" | `dispatcher.rs:193-221`；异步路径无 `save_contact_event` | ⏸ 待修 |
| F-M3 | 🔴 | `ContactRecord` 无 `owner_type`，trait 实现硬编码 `OwnerType::User`，Agent 数据被写成 User | `store.rs:758` | ⏸ 待修 |
| F-M4 | 🟠 | 异步写三库无回滚/事务，失败留部分写入 | `background_queue.rs::write_extraction_results` | ⏸ 待修 |
| F-M5 | 🟠 | 同步 `DataDispatcher` 与异步 `write_extraction_results` 两套写路径重复实现 | 架构债 | ⏸ 待修 |
| F-M6 | 🟡 | `completed` 状态写回错误被 `let _ =` 忽略 | `background_queue.rs:279` | ⏸ 待修 |
| F-M7 | 🟡 | `activity_score` 在 `ContactRecord` 转换时固定 0.0，活跃度只存关系库 | `store.rs:792` | ⏸ 待修 |

---

## 模块 2：待办管理 ✅

### 核对结论

- 5 个端点齐全：`POST /api/todos`、`GET /api/todos`、`GET /api/todos/:id`、`PUT /api/todos/:id/complete`、`DELETE /api/todos/:id`。
- 真实 `TodoItem`（`todo_list.rs:12`）与 `FEATURE_MODULES_CN` 一致：`description: Option<String>`、`deadline: DateTime<Utc>`（非可选）、`status` / `importance` 为 `String`、`owner_type: OwnerType`。
- 唯一索引存在：`store.rs:332` `idx_unique_pending_todo (title, owner_id, owner_type, deadline) WHERE status='pending'`；`add_todo` 用 `INSERT OR IGNORE` + `rows_affected()==0` 返回 `ConflictError`。
- 归档：`archive_stale_todos` 只归档 `status='pending' AND is_archived=0 AND last_updated_at < now-30d`，按 `last_updated_at ASC` 限量，符合"仅归档 30 天以上"的设计。
- 提醒：`insert_reminder_todo` 生成 `todo_type='reminder'`、`importance='low'`、当日 23:59:59 deadline 的提醒；`has_recent_reminder` 按 `todo_type='reminder' AND created_at >= today` 去重。
- 过期：`invalidate_expired_todos` 把 `pending AND is_archived=0 AND deadline < now` 改为 `auto_invalid`。
- 写入时归档/提醒逻辑在 `heartbeat/executor.rs:126-154` 与 `api/async_memory.rs:372-397` **各实现了一遍**。

### 问题（F-T）

| 编号 | 严重度 | 问题 | 证据 | 状态 |
|------|--------|------|------|------|
| F-T1 | 🔴 | Agent 作用域 owner_id 系统性硬编码为 `"primary_agent"`，与配置默认 `"primary_agent_default"` 不一致 | `core/contact.rs:61`、`routes.rs:1171`、`routes.rs:455`、`internal_api.rs:46` | ⏸ 待修 |
| F-T2 | 🔴 | `resolve_owner` 只推断 `User` 所有者；未传 scope 时对 Agent 待办回退为 `(User, "default_user")`，导致 Agent 待办查不到 | `routes.rs:1345-1360` | ⏸ 待修 |
| F-T3 | 🟠 | 待办阈值 30 / 50 与 archive `keep_count=50` 在两处硬编码，且无配置项 | `heartbeat/executor.rs:129-147`、`api/async_memory.rs:375-391` | ⏸ 待修 |
| F-T4 | 🟠 | `update_todo_deadline` 只改 `deadline`，不更新 `last_updated_at`；刚改期的待办仍可能被判为陈旧并归档 | `todo_list.rs:589-602` | ⏸ 待修 |
| F-T5 | 🟡 | 应用层去重用"分钟精度"，唯一索引用精确 deadline 字符串，两套语义不一致 | `routes.rs:1199-1208` vs `store.rs:332` | ⏸ 待修 |
| F-T6 | 🟡 | 提醒的"当日 23:59:59"用 UTC 计算，`has_recent_reminder` 也用 UTC 日界；UTC+8 下日界为本地 08:00 | `todo_list.rs:681, 727-730` | ⏸ 待修 |
| F-T7 | 🟡 | `schema.sql` 的 `todo_list` 缺 v0.5.1 列（`todo_type/entity_type/entity_id/is_archived/archived_at/last_updated_at/resolved_*`），实际靠 `store.rs:410-453` 的 ALTER 补齐 | `schema.sql:39-52` vs `store.rs:410-453` | ⏸ 待修（DDL 同步） |
| F-T8 | 🟡 | `TECHNICAL_DOCUMENTATION` 与 `EXTENSION_GUIDE` 的 `TodoItem` 字段写反：写 `description: String` / `deadline: Option`，与代码相反 | `TECHNICAL_DOCUMENTATION:280-300`、`EXTENSION_GUIDE:151-169` | ⏸ 待修（文档） |

---

## 模块 3：心跳系统 ✅

### 核对结论

- 4 个端点齐全；`trigger` 支持可选 `maintenance_only`，`ConcurrencyError` 正确映射为 **409**。
- `ExecutingGuard`（RAII）确实存在并在 `trigger_now` 中使用，panic/超时也能清 `executing` 标志。
- 维护阶段顺序正确：`cleanup_expired_pending_merges → promote_demote_events → cleanup_expired_events/low_importance → 孤立向量清理 → 联系人归档(180d) → 待办失效/清理 → 待办归档(>50)/提醒(>30)`。
- 孤立向量清理已实现（仅事件向量）；todo `auto_invalid`、提醒去重、`MAX_PROFILE_UPDATES_PER_DAY` 频率限制都在。
- LLM 输出 `extract_json_from_response` 兼容 markdown 代码块/截断 JSON；`social_relations` 兼容字符串数组与对象数组。

### 设计/文档问题（D-H）

| 编号 | 严重度 | 问题 | 证据 | 状态 |
|------|--------|------|------|------|
| D-H1 | 🟠 | 维护阶段内容与文档不符：文档称含"事件去重、联系人合并、歧义检测（无 LLM）"，实际这些在 analyzer 的 LLM 阶段；维护阶段只有清理/生命周期/归档 | `executor.rs:61-184` vs `analyzer.rs:166-178` | ⏸ 待修（文档/设计对齐） |
| D-H2 | 🟡 | `QuestionPool` 实际用 **sled** 持久化；文档称"SQLite + 文件双存储"、`questions` 表 | `question_pool.rs:7,24-44` | ⏸ 待修（文档） |
| D-H3 | 🟡 | `HEARTBEAT_V0.5.md:189` 称 `[heartbeat]` 配置未被使用；实际 server 已读取（但 min/max/adaptive/idle 仍未生效） | `shouxin-server.rs:490-497` | ⏸ 待修（文档） |
| D-H4 | 🟡 | `HeartbeatStatus` 字段与文档不一致：文档有 `heartbeat_count/events_merged/contacts_merged/ambiguities_detected/profiles_updated`，实际没有 | `scheduler.rs:56-67` | ⏸ 待修（文档/接口） |
| D-H5 | 🟢 | `HeartbeatReport` 字段类型：`FEATURE_MODULES_CN` 写 `usize`（正确），`HEARTBEAT_V0.5` 写 `u32`（错误） | `types.rs:48-65` | ⏸ 待修（文档） |
| D-H6 | 🟡 | 默认间隔三套不一致：config.toml 60/30/120/30；`settings.rs` 1800/600/3600/adaptive=true/300；`SchedulerConfig::default` 360/300/600/false/300；文档 360/300/600/30 | 三处 + 文档 | ⏸ 待修（配置收敛） |

### 功能问题（F-H）

| 编号 | 严重度 | 问题 | 证据 | 状态 |
|------|--------|------|------|------|
| F-H0 | 🔴 | `settings.heartbeat.enabled` **从未被读取**：服务无条件启动调度器，`enabled=false` 无效 | 全 `src` 无 `heartbeat.enabled` 读取；`shouxin-server.rs:499-505` 无条件 spawn | ⏸ 待修 |
| F-H1 | 🔴 | `min_interval_secs` / `max_interval_secs` / `adaptive` / `idle_threshold_secs` 传入后从未使用；`run()` 只按 `base_interval_secs` 固定睡眠，自适应/空闲检测未实现 | `scheduler.rs:126-127`；其余字段仅定义/赋值 | ⏸ 待修 |
| F-H2 | 🟠 | `storage.question_pool_dir` 配置未使用，server 硬编码 `.heartbeat_data` | `settings.rs:193`、`shouxin-server.rs:479` | ⏸ 待修 |
| F-H3 | 🟠 | 超时全部硬编码：维护 30s、LLM 120s、analyzer 子调用 90s、歧义 120s | `scheduler.rs:227,253,276`、`analyzer.rs:275,661,887` | ⏸ 待修 |
| F-H4 | 🟠 | 阈值硬编码：内存 4096MB、最大跳过 3、联系人归档批量 1000、待办 30/50、画像每日 2 次 | `scheduler.rs:17-18`、`executor.rs:99,129-147`、`analyzer.rs:22` | ⏸ 待修 |
| F-H5 | 🟡 | `run_heartbeat` 无调用方（死兼容方法）；LLM 失败时返回 maintenance 报告且 `maintenance_only=true`，客户端可能误判"只跑了维护" | `executor.rs:44-58`、`scheduler.rs:315` | ⏸ 待修 |
| F-H6 | 🟡 | `get_pending_questions(usize::MAX)` 无上限，问题池大时全量加载进内存 | `routes.rs:1396`、`scheduler.rs:333` | ⏸ 待修 |
| F-H7 | 🟡 | `next_heartbeat_in_secs` 始终返回 `base_interval_secs`，未扣除已过时间 | `scheduler.rs:346` | ⏸ 待修 |
| F-H8 | 🟡 | 联系人归档单次批量上限 1000 写死，联系人多时需多轮才能归档完 | `executor.rs:99` | ⏸ 待修 |

---

## 模块 4：文件管理 ✅

### 核对结论

- 4 个端点齐全：`POST /api/files/upload`、`GET /api/files/:file_id`、`GET /api/files/:file_id/text`、`DELETE /api/files/:file_id`。
- 上传强制 `Content-Type: application/octet-stream`，拒绝 multipart；空 body 返回 400；>20MB 返回 413。
- MD5 去重按 `(md5_hash, owner_type, owner_id)`，`file_store` 有对应唯一索引；命中时返回 `is_duplicate: true`。
- 异步化已实现：DB 同步插入 `pending` → `tokio::spawn` 处理 → `processing → completed/failed`。
- 文本提取支持 txt/md/code、pdf、docx（zip XML）、xlsx（zip XML）、csv；成功后删除临时文件。
- `delete_file` 同时删除 `file_index` 与 `file_relations`；`FileIndex` 字段与文档一致。

### 设计/文档问题（D-F）

| 编号 | 严重度 | 问题 | 证据 | 状态 |
|------|--------|------|------|------|
| D-F1 | 🟡 | 文档称 `filename` 可选（默认 `upload.txt`），实际 `UploadFileQuery.filename: String` 为必填 | `routes.rs:442-447`、`FEATURE_MODULES_CN:393-395` | ⏸ 待修（文档） |
| D-F2 | 🟡 | 上传响应字段与文档不一致：文档 `status`/`dedup`，实际 `processing_status`/`is_duplicate` | `routes.rs:585-594,654-663` | ⏸ 待修（文档） |
| D-F3 | 🟡 | `PROJECT_INTRO` 写 `GET /api/files/list`，实际无此端点 | `routes.rs:727-731` | ⏸ 待修（文档） |
| D-F4 | 🟡 | 临时目录文档写 `./temp_uploads`，实际 `<data_dir>/temp_uploads` | `routes.rs:601` | ⏸ 待修（文档） |
| D-F5 | 🟡 | 文档称支持 `doc/xls`，实际只支持 `docx/xlsx`；旧版 `.doc/.xls` 被 `detect_file_type` 归为 `txt`，会按文本读取二进制 | `file_processor.rs:150-175` | ⏸ 待修（文档/实现） |
| D-F6 | 🟢 | `owner_type` 存小写 scope 字符串（`user`/`agent`），其他模块存 `User`/`Agent`，跨模块口径不一致 | `routes.rs:616`、`todo_list.rs:247` | ⏸ 待修（数据规范） |

### 功能问题（F-F）

| 编号 | 严重度 | 问题 | 证据 | 状态 |
|------|--------|------|------|------|
| F-F1 | 🟠 | 并发重复上传竞态：`insert_file` 用 `INSERT OR IGNORE` 但不检查 `rows_affected`；冲突时 handler 已写临时文件并返回新 `file_id`，DB 无记录、临时文件成孤儿 | `file_store.rs:112-139`、`routes.rs:597-629` | ⏸ 待修 |
| F-F2 | 🟠 | 删除文件不删临时文件：`delete_file` 只删 DB + 字典文本，`pending/failed` 的 `original_path` 留在磁盘 | `routes.rs:702-714` | ⏸ 待修 |
| F-F3 | 🔴 | `get_file_metadata` / `get_file_text` / `delete_file` 均无 owner 校验：任意 `file_id` 可读/删，跨 owner 越权 | `routes.rs:666-714` | ⏸ 待修 |
| F-F4 | 🟠 | 无重启恢复：后台 `tokio::spawn` 任务丢失，`pending` 文件重启后永远 pending | `routes.rs:642-651` | ⏸ 待修 |
| F-F5 | 🟠 | 20MB 检查在完整 body 缓冲之后（file 路由禁用 body limit），大文件先占内存再拒绝 | `routes.rs:549-558,733` | ⏸ 待修 |
| F-F6 | 🟠 | `owner_type` 口径不一致（小写 scope vs `OwnerType` Debug），影响跨模块按 owner 查询/去重 | `routes.rs:616` | ⏸ 待修 |
| F-F7 | 🟡 | `user_id` 默认 `"primary_agent"` 硬编码（与 F-T1 同源） | `routes.rs:455,447` | ⏸ 待修 |
| F-F8 | 🟠 | 删除文件不删向量：chunk 以 `source_file_id` metadata 写入向量库，`delete_file` 未按该 metadata 清理，留下孤儿向量 | `file_processor.rs:109-119`、`routes.rs:702-714` | ⏸ 待修 |
| F-F9 | 🟡 | `process_file` 失败路径返回 `Ok(())`，上传侧日志打印 "processed successfully"，误导排查 | `file_processor.rs:75-90`、`routes.rs:644-646` | ⏸ 待修 |
| F-F10 | 🟡 | `get_file_metadata` 返回 `original_path` 绝对服务器路径，信息泄露 | `routes.rs:670-671` | ⏸ 待修 |
| F-F11 | 🟠 | `.doc/.xls` 实际不支持却被当 `txt` 接受，提取阶段读二进制乱码/失败 | `file_processor.rs:173-174` | ⏸ 待修 |
| F-F12 | 🟡 | `insert_file_relation` 失败只 `eprintln`，不返回错误，关联可能静默丢失 | `routes.rs:631-635` | ⏸ 待修 |
| F-F13 | 🟡 | 文本按 chunk 串行逐条 `embed`，大文件处理慢 | `file_processor.rs:102-126` | ⏸ 待优化 |

---

## 模块 5：虚拟实体管理 ✅

### 核对结论

- 6 个端点齐全：list / get / add / update / delete / check。
- 内置 **11 个**虚拟角色（林克、ChatGPT、马里奥、皮卡丘、孙悟空、曹操、蝙蝠侠、钢铁侠、克劳德、哆啦A梦、柯南），启动时按名称去重初始化；`owner_id="system"`、`is_virtual=true`、别名逐条写入，符合文档。
- 用户创建：`is_virtual=true`、`owner_type=User`、`owner_id="default_user"`，符合文档。
- 名称匹配：先查 `virtual_character_aliases`（`LOWER(alias_name)`），未命中再查 `contact_index.name AND is_virtual=1`，均不区分大小写，符合文档。
- 删除：先删别名，再 `DELETE FROM contact_index WHERE id=? AND is_virtual=1`，有 `is_virtual` 保护。
- 响应结构是 P0-3 的"API 规范兼容"扁平结构（`entity_id/name/aliases/created_at/updated_at`），可用但与文档不一致。

### 设计/文档问题（D-V）

| 编号 | 严重度 | 问题 | 证据 | 状态 |
|------|--------|------|------|------|
| D-V1 | 🟡 | `check` 未命中时返回 `success(null)`，文档称"否则返回错误" | `routes.rs:1777-1790` | ⏸ 待修（文档/契约） |
| D-V2 | 🟡 | 响应结构与文档不一致：文档 `VirtualCharacterInfo { contact, aliases }`，实际扁平 `VirtualEntityResponse`；`AliasResponse` 无 `id/contact_id/created_at` | `routes.rs:1562-1578` | ⏸ 待修（文档） |
| D-V3 | 🟡 | 文档称 `ContactIndex.aliases` 承载别名；实际 add 只写 `[name]`，别名仅在 `virtual_character_aliases` 表 | `routes.rs:1659`、`shouxin-server.rs:272` | ⏸ 待修（数据一致性） |

### 功能问题（F-V）

| 编号 | 严重度 | 问题 | 证据 | 状态 |
|------|--------|------|------|------|
| F-V1 | 🟠 | `update` 别名替换无事务：先 `delete_aliases` 再逐条 `insert`，两者都用 `.ok()` **吞掉错误**；插入失败或中途崩溃 → 别名全丢 | `routes.rs:1719-1732` | ⏸ 待修 |
| F-V2 | 🟠 | `add` 别名插入失败只 `warn!`，API 仍返回成功；联系人已建但别名缺失 | `routes.rs:1683-1693` | ⏸ 待修 |
| F-V3 | 🟡 | `delete` 先删别名再删联系人；联系人不存在时别名已被删；且路由内直接写 SQL，绕过 store 层 | `routes.rs:1749-1771` | ⏸ 待修 |
| F-V4 | 🟡 | `ContactIndex.aliases` JSON 不随别名/名称更新：add 只写 `[name]`，update 改名后仍是旧名；依赖该 JSON 的 `find_contact_by_any_name` 可能匹配不到 | `routes.rs:1659,1714-1716` | ⏸ 待修 |
| F-V5 | 🟡 | 重复名称创建：`upsert_contact_index` 是 `ON CONFLICT(id)`，同名同 owner 会触发唯一索引冲突，API 返回"内部错误"而非 409/友好提示 | `store.rs:567-590` | ⏸ 待修 |
| F-V6 | 🟡 | `virtual_character_aliases` 无唯一约束，重复 add 会产生重复别名 | `store.rs:518-547` | ⏸ 待修 |
| F-V7 | 🟡 | 孤立别名导致 `check_virtual_match` 提前返回 `None`：别名存在但联系人已删时，不再回退查联系人名 | `virtual_entity.rs:68-93` | ⏸ 待修 |
| F-V8 | 🟡 | `get`/`check` 未命中返回 200 `success(null)`，非 404 | `routes.rs:1630-1645,1777-1790` | ⏸ 待修（契约） |
| F-V9 | 🟡 | 无 owner/scope 校验，虚拟实体全局可改/删（未来多用户需隔离） | `routes.rs:1705-1771` | ⏸ 待修 |
| F-V10 | 🟡 | list 无分页，实体多时全量返回 | `routes.rs:1614-1628` | ⏸ 待优化 |
| F-V11 | 🟢 | 内置角色按名称去重；若用户先创建同名角色，内置别名不会补齐 | `shouxin-server.rs:256-266` | ⏸ 待评估 |

---

## 模块 6：事件合并管理 ✅

**结论**：2 个端点齐全；自动合并 >0.90、待确认 0.75–0.90、owner 过滤、向量目标存在性校验都已实现；`confirm` 校验 action 与 `status='pending'`。

| 编号 | 严重度 | 问题 | 证据 |
|------|--------|------|------|
| D-E1 | 🟡 | 文档说向量检索 top 3，实际 `search(..., 5, None)` top 5 | `background_queue.rs:466` |
| F-E1 | 🟠 | 合并阈值 0.75 / 0.90 硬编码，无配置项 | `background_queue.rs:499,513` |
| F-E2 | 🟡 | `event_similarity_cache` 表已建但全代码未使用（死表） | 迁移 `v0_5_1_event_semantic_merge.sql:34`，`src` 无引用 |
| F-E3 | 🟡 | `confirm_pending_event_merge` 只按 `merge_id`，无 owner 校验，任意调用者可确认任意记录 | `store.rs:2846-2874` |
| F-E4 | 🟡 | 确认合并多步写入无事务（读→改事件→插入→改状态），失败可能半完成 | `store.rs:2884+` |
| F-E5 | 🟡 | `owner_type` 非法值静默降级为 `User` | `routes.rs:957-963` |
| F-E6 | 🟠 | 自动合并失败只 `warn` 后 `continue`，源事件既不合并也不创建 → **数据丢失** | `background_queue.rs:524-531` |
| F-E7 | 🟠 | 待确认记录插入失败只 `warn` 后 `continue`，源事件同样丢失 | `background_queue.rs:540-551` |

---

## 模块 7：向量搜索 ✅

**结论**：`/api/vector/search` 与 `/api/memory/search/semantic` 共用一个 handler；768 维；按 owner/source_types/conversation_id 过滤；owner 过滤为空时会回退无 owner 再搜一次。

| 编号 | 严重度 | 问题 | 证据 |
|------|--------|------|------|
| D-VS1 | 🟡 | 文档说 embedded 后端是 "内存 usearch"，实际是暴力余弦扫描 | `stores/vector/store.rs` |
| F-VS1 | 🟡 | `conversation_id` 过滤在 `limit` 截断**之后**执行，可能返回空而实际有匹配 | `routes.rs:1080-1086` |
| F-VS2 | 🟡 | `GET /api/vector/:id` 无 owner 校验 | `routes.rs:1099-1112` |
| F-VS3 | 🟡 | 768 维在 `new()` / `in_memory()` 等默认构造中硬编码 | `store.rs:37,81`、`hnsw_store.rs:657` |
| F-VS4 | 🟢 | 两个路由指向同一 handler，属有意设计但增加维护歧义 | `routes.rs:762-767` |
| F-VS5 | 🟠 | owner 过滤为空时回退到**无 owner 过滤**搜索，可能跨 owner 返回数据 | `three_library_manager.rs:980` |

---

## 模块 8：智能搜索 ✅

**结论**：4 类来源（contacts/events/todos/history）聚合；默认全部类型；每类 `min(limit,20)`；contacts/events 走带 query 的搜索。

| 编号 | 严重度 | 问题 | 证据 |
|------|--------|------|------|
| F-SS1 | 🟠 | `history` 分支过滤条件是 `true`，**完全不做关键词匹配**，只返回最近 30 天前 N 条 | `routes.rs:1520-1527` |
| F-SS2 | 🟡 | 各来源错误被 `if let Ok(...)` 吞掉，部分失败仍返回 200 与空数组 | `routes.rs:1487-1528` |
| F-SS3 | 🟡 | todos 全量取出后在内存 `to_lowercase().contains()` 过滤，O(n) 且对中文 lower 无效（contains 仍可用） | `routes.rs:1499-1510` |
| D-SS1 | 🟡 | 文档称 history "在内存中过滤"，实际未过滤 | `FEATURE_MODULES_CN:765` vs 代码 |

---

## 模块 9：内部 API ✅

**结论**：2 个端点；`Authorization: Bearer` + `constant_time_eq`；未配置 `INTERNAL_API_TOKEN` 返回 500；注册返回 201/409；内部事件解析时间并计算 `auto_invalid_at`。

| 编号 | 严重度 | 问题 | 证据 |
|------|--------|------|------|
| D-I1 | 🟡 | 文档称注册同时创建 `ContactIndex` + `ContactTotal`；实际只 `upsert_contact_index`，`ContactTotal` 构造后丢弃 | `internal_api.rs:173-213` |
| D-I2 | 🟡 | 配置注释称 token 可来自 `[agent] internal_api_token`；内部 API 只读 `INTERNAL_API_TOKEN` 环境变量，配置项对该端点无效 | `internal_api.rs:90-98` |
| F-I1 | 🟡 | `default_owner_id()` 硬编码 `"primary_agent"`，与配置 `primary_agent_id` 不一致 | `internal_api.rs:45-47` |
| F-I2 | 🟡 | 内部事件只写关系库，不写字典库，与"字典库存事件历史"的设计不一致 | `internal_api.rs:260` |
| F-I3 | 🟡 | `importance="medium"`、`importance_score=0.5` 硬编码 | `internal_api.rs:240,254` |
| F-I4 | 🟡 | `register_agent` 无 agent_id/name 空值与长度校验 | `internal_api.rs:137-171` |

---

## 模块 10：管理 API ✅

**结论**：2 个端点；`Authorization: Bearer` + 常量时间比较；空 token 时拒绝所有请求；支持手动切换 primary/secondary。

| 编号 | 严重度 | 问题 | 证据 |
|------|--------|------|------|
| D-A1 | 🟡 | 文档写 `X-Internal-Token` 头，实际统一为 `Authorization: Bearer`（S-06） | `shouxin-server.rs:606-609` vs 文档 |
| D-A2 | 🟡 | 状态响应缺 `health_check_interval_secs`（文档 ManagerStatus 有） | `shouxin-server.rs:576-586` |
| F-A1 | 🟡 | `switch` 失败返回 HTTP 200 + `success:false`，非 4xx | `shouxin-server.rs:594-597` |
| F-A2 | 🟡 | 无速率限制（安全审计 #7 仍未做） | 全局无 rate limiter |
| F-A3 | 🟡 | token 为空时仅 `warn`，服务照常启动，所有管理请求 401；易漏配 | `shouxin-server.rs:563-567` |

---

## 模块 11：健康检查 ✅

**结论**：`GET /health` 返回静态 `{status:"ok", time}`，无需认证，与文档一致。无 DB/LLM 依赖探测（文档也未承诺），列为可选增强。

---

## 模块 12：LLM 系统 ✅

**结论**：三后端（GGUF/Ollama/API）；主备切换、连续失败自动切换、后台健康检查、模型预加载、指数退避（2 次）均已实现。

| 编号 | 严重度 | 问题 | 证据 |
|------|--------|------|------|
| F-L1 | 🟠 | GGUF 内部超时硬编码 **180s**，与 `[llm] timeout_secs=600` 不一致，实际 180s 就断 | `llm/gguf.rs:387`、`config.toml:8` |
| F-L2 | 🟠 | Ollama 用 `DEFAULT_TIMEOUT_SECS` 常量，未用 `[llm.ollama] timeout_secs`；`ApiClient.timeout_secs` 字段未被读取 | `llm/ollama.rs:111`、`llm/api.rs:63` |
| F-L3 | 🟠 | `max_consecutive_failures=3`、`health_interval_secs=30` 硬编码在工厂 | `llm/factory.rs:176-177` |
| F-L4 | 🟡 | 重试固定 2 次、退避 2^n×1000ms 硬编码 | `llm/manager.rs:273,317` |

---

## 模块 13：三库存储 ✅

**结论**：Sled 字典库、SQLite WAL 关系库、HNSW/Embedded 向量库；SQLite `max_connections=64`、`busy_timeout=5000`；`SHOUXIN_STORE_TYPE` 选择 embedded/hnsw；HNSW 关闭时持久化；graceful shutdown flush+close。

| 编号 | 严重度 | 问题 | 证据 |
|------|--------|------|------|
| D-S1 | 🟡 | 文档 `vector_dir = "vector"`，实际配置 `vector_store` | 文档 vs `config.toml:53` |
| F-S1 | 🟠 | 同步/异步写路径不一致（已由 F-M1/F-M2/F-X4/F-X5 具体化） | 见模块 1/14 |
| F-S2 | 🟡 | `store.rs` 内两处 `max_connections(20)`，与文档/服务端 64 不一致 | `store.rs:1503,1516` |
| F-S3 | 🟠 | 三库写入无跨库事务，失败留部分写入 | 见 F-M4 |
| — | ✅ | WAL / busy_timeout / owner 列 / HNSW 持久化 / 优雅关闭均符合文档 | — |

---

## 模块 14：记忆提取器 ✅

**结论**：LLM 提取 + JSON 清洗 + 降级正则；截断 1024 字符；乱码检测 5%/30%；姓名截断修复；多段提取；质量阈值 0.8；同步路径 0.98 去重。

| 编号 | 严重度 | 问题 | 证据 |
|------|--------|------|------|
| F-X1 | 🟡 | 对话截断 1024 字符硬编码 | `extractor.rs:362` |
| F-X2 | 🟡 | 乱码阈值 0.05 / 0.30 硬编码 | `extractor.rs:196-197` |
| F-X3 | 🟡 | 质量阈值 0.8 在 dispatcher 与 manager 两处硬编码 | `dispatcher.rs:294`、`three_library_manager.rs:790` |
| F-X4 | 🟠 | **异步路径无 0.98 学习素材去重**（仅同步 `three_library_manager` 有） | `background_queue.rs:770-815` vs `three_library_manager.rs:796-811` |
| F-X5 | 🟠 | **异步路径无 `quality_score >= 0.8` 过滤**，低质量素材也写入向量库 | `background_queue.rs:770-815` |

---

## 累积待修总表

> 模块 1-5 见本文件上方各节；模块 6-14 的完整条目已汇总到 **`FIX_LIST.md`**（按 P0/P1/P2 重排）。

| 模块 | 编号 | 严重度 | 一句话 |
|------|------|--------|--------|
| 记忆管理 | F-M1 | 🔴 | 字典库联系人业务字段被异步写回清空 |
| 记忆管理 | F-M2 | 🔴 | 字典库缺联系人事件历史 |
| 记忆管理 | F-M3 | 🔴 | Agent 联系人被写成 User |
| 记忆管理 | F-M4 | 🟠 | 异步写三库无回滚 |
| 记忆管理 | F-M5 | 🟠 | 同步/异步双写路径重复 |
| 记忆管理 | D-M4 | 🟠 | MAX_ACTIVE_CONTACTS 双处硬编码 |
| 记忆管理 | D-M5 | 🟠 | worker 嵌套 runtime / expect |
| 记忆管理 | D-M6 | 🟡 | new() 静默丢任务 |
| 记忆管理 | D-M7 | 🟡 | .cargo 旧路径 |
| 记忆管理 | D-M8 | 🟡 | flush 性能待评估 |
| 记忆管理 | F-M6 | 🟡 | completed 状态写回忽略错误 |
| 记忆管理 | F-M7 | 🟡 | activity_score 丢失 |
| 待办管理 | F-T1 | 🔴 | Agent owner 硬编码 primary_agent |
| 待办管理 | F-T2 | 🔴 | resolve_owner 无法推断 Agent |
| 待办管理 | F-T3 | 🟠 | 30/50 阈值双处硬编码 |
| 待办管理 | F-T4 | 🟠 | 改 deadline 不更新 last_updated_at |
| 待办管理 | F-T5 | 🟡 | 去重语义两套不一致 |
| 待办管理 | F-T6 | 🟡 | UTC 日界与本地日界不一致 |
| 待办管理 | F-T7 | 🟡 | schema.sql 滞后 |
| 待办管理 | F-T8 | 🟡 | TodoItem 文档字段写反 |
| 心跳系统 | F-H0 | 🔴 | heartbeat.enabled 从未读取，关不掉心跳 |
| 心跳系统 | F-H1 | 🔴 | min/max/adaptive/idle 配置未生效，自适应未实现 |
| 心跳系统 | D-H1 | 🟠 | 维护阶段内容与文档不符（去重/合并/歧义实际在 LLM 阶段） |
| 心跳系统 | F-H2 | 🟠 | question_pool_dir 未使用，硬编码 .heartbeat_data |
| 心跳系统 | F-H3 | 🟠 | 维护/LLM/analyzer 超时全部硬编码 |
| 心跳系统 | F-H4 | 🟠 | 内存/跳过次数/归档批量/画像次数等阈值硬编码 |
| 心跳系统 | D-H2 | 🟡 | QuestionPool 存储文档错误（sled 写成 SQLite） |
| 心跳系统 | D-H3 | 🟡 | 文档称配置未使用，实际已读取 |
| 心跳系统 | D-H4 | 🟡 | HeartbeatStatus 字段与文档不一致 |
| 心跳系统 | D-H5 | 🟢 | HeartbeatReport 字段类型文档不一致 |
| 心跳系统 | D-H6 | 🟡 | 默认间隔三套值不一致 |
| 心跳系统 | F-H5 | 🟡 | run_heartbeat 无调用方 / LLM 失败标记误导 |
| 心跳系统 | F-H6 | 🟡 | 待处理问题无上限全量加载 |
| 心跳系统 | F-H7 | 🟡 | next_heartbeat_in_secs 未扣已过时间 |
| 心跳系统 | F-H8 | 🟡 | 联系人归档批量 1000 写死 |
| 文件管理 | F-F3 | 🔴 | get/delete 无 owner 校验，跨 owner 越权 |
| 文件管理 | F-F1 | 🟠 | 并发重复上传竞态，孤儿临时文件 + 无效 file_id |
| 文件管理 | F-F2 | 🟠 | 删除不清理临时文件 |
| 文件管理 | F-F4 | 🟠 | pending 文件无重启恢复 |
| 文件管理 | F-F5 | 🟠 | 20MB 检查在完整缓冲后，内存 DoS |
| 文件管理 | F-F6 | 🟠 | owner_type 口径不一致 |
| 文件管理 | F-F8 | 🟠 | 删除文件不清理向量，孤儿向量 |
| 文件管理 | F-F11 | 🟠 | .doc/.xls 不支持却被当 txt 接受 |
| 文件管理 | D-F1~D-F5 | 🟡 | 文档偏差（filename/响应字段/files list/临时目录/doc-xls） |
| 文件管理 | F-F7 | 🟡 | user_id 默认 primary_agent 硬编码 |
| 文件管理 | F-F9 | 🟡 | 失败返回 Ok，日志误导 |
| 文件管理 | F-F10 | 🟡 | 元数据泄露绝对路径 |
| 文件管理 | F-F12 | 🟡 | 关联插入失败静默 |
| 文件管理 | F-F13 | 🟡 | 串行 embed 慢 |
| 文件管理 | D-F6 | 🟢 | owner_type 小写/大小写口径不一致 |
| 虚拟实体 | F-V1 | 🟠 | update 别名替换无事务、错误被吞，可能全丢 |
| 虚拟实体 | F-V2 | 🟠 | add 别名失败仍返回成功 |
| 虚拟实体 | D-V1 | 🟡 | check 未命中返回 null 而非错误 |
| 虚拟实体 | D-V2 | 🟡 | 响应结构与文档不符 |
| 虚拟实体 | D-V3 | 🟡 | ContactIndex.aliases 未承载别名 |
| 虚拟实体 | F-V3 | 🟡 | delete 先删别名后删联系人，非原子 |
| 虚拟实体 | F-V4 | 🟡 | aliases JSON 不随别名/改名更新 |
| 虚拟实体 | F-V5 | 🟡 | 重名创建返回"内部错误" |
| 虚拟实体 | F-V6 | 🟡 | 别名表无唯一约束 |
| 虚拟实体 | F-V7 | 🟡 | 孤立别名导致匹配提前返回 None |
| 虚拟实体 | F-V8 | 🟡 | get/check 未命中 200 null |
| 虚拟实体 | F-V9 | 🟡 | 无 owner 校验 |
| 虚拟实体 | F-V10 | 🟡 | list 无分页 |

> 说明：D-M1/D-M2/D-M3/D-M9 已完成，不列入待修总表。
