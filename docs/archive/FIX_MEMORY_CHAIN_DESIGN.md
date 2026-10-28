# 记忆管理全链路 · 设计核对报告

> 日期：2026-10-27
> 范围：第 1 条链路「记忆管理」——`POST /api/memory/add → 队列 → 提取 → 分发 → 三库 → 状态查询`
> 方法：设计文档 vs 真实代码逐点对照（未运行）
> 版本口径：0.5.0.3 / 文档 v0.5.3

---

## 一、核对依据

**设计侧**
- `ASYNC_EXTRACT_DESIGN.md`（异步提取架构设计，v0.5.1）
- `docs/TECHNICAL_DOCUMENTATION.md` §3 三库架构、§6.2 提取流水线
- `docs/FEATURE_MODULES_CN.md` 模块 1「记忆管理」
- `docs/API_REFERENCE.md` 记忆管理 API（6 个端点）

**实现侧**
- `src/api/routes.rs`（`add_conversation` / `extract_status`）
- `src/api/async_memory.rs`
- `src/memory/three_library_manager.rs`（`add_async`）
- `src/service/background_queue.rs`（`push_with_persist` / `recover_pending_tasks` / `process_task`）
- `src/memory_extractor/extractor.rs`、`dispatcher.rs`
- `src/shouxin-server.rs`（装配与恢复）

---

## 二、设计预期 vs 实际实现

| # | 设计预期 | 实现 | 结论 |
|---|----------|------|------|
| 1 | `add` 立即返回，不等待 LLM | `add_async` 保存后即返回 `conversation_id` | ✅ 一致 |
| 2 | 先落字典库，再入队 | `save_conversation` → `push_with_persist` | ✅ 一致 |
| 3 | 队列持久化、重启恢复 | sled `pending_extraction_tasks` 树 + `recover_pending_tasks()`（server:467） | ✅ 已实现 |
| 4 | 状态 `pending/processing/completed/failed` | 状态字符串一致 | ✅ 一致 |
| 5 | 状态可查询 | `GET /api/memory/extract_status/:id`，`content_preview` 用 `chars().take(200)`（UTF-8 安全） | ✅ 一致，且比设计片段更安全 |
| 6 | LLM 失败/超时降级正则 | `fallback_extract` 双路径 | ✅ 已实现 |
| 7 | 失败自动重试 | `retry_count` 从未递增；失败即从 sled 删除 | ⚠️ 按决策**不实现**，文档已修正（D-M2） |
| 8 | 零数据丢失 | 原文已入库；入队失败现已标记 `failed` 并记录原因 | ✅ 已修（D-M1） |
| 9 | worker 为 async task、共享 runtime、不嵌套 | 实际用 `spawn_blocking` + 新建 `current_thread` runtime + `.expect()` | ❌ **与设计相反** |
| 10 | 600s 提取超时 | 两处硬编码 `from_secs(600)` | ⚠️ **已修复为配置** |

---

## 三、设计级问题清单（D-M）

| 编号 | 严重度 | 问题 | 证据 | 影响 | 修复方向 |
|------|--------|------|------|------|----------|
| **D-M1** | 🔴 | 入队失败被吞：`push_with_persist` 返回 Err 时只 `error!` 日志，仍返回 `Ok(conversation_id)` | `three_library_manager.rs:259-261` | 客户端以为成功，实际提取任务丢失；违背"零丢失"承诺 | 向上传播错误，或返回 `queued: false` + 状态标 `failed` |
| **D-M2** | 🔴 | 无自动重试：`retry_count` 从未递增；失败直接删 sled 记录 | `background_queue.rs:239-267` | `PROJECT_INTRO` 宣称的"失败自动重试"不存在；瞬时故障即永久失败 | 增加可配置 `max_retries` + 退避；或修正文档 |
| **D-M3** | 🟠 | 未配置队列时静默跳过提取，仍返回成功 | `three_library_manager.rs:267-269` | 装配漏配时功能静默失效 | 启动期校验，或 `add` 返回明确状态 |
| **D-M4** | 🟠 | `MAX_ACTIVE_CONTACTS=50` 两处硬编码，注释要求手工同步 | `background_queue.rs:17`、`dispatcher.rs:36` | 改一处漏一处，热表行为不一致 | 收敛到 `[extraction]` 或 `[contact]` 配置，单一来源 |
| **D-M5** | 🟠 | worker 每个任务 `spawn_blocking` + 新建 `current_thread` runtime + `.expect()` | `background_queue.rs:163-171` | 与设计文档"共享主 runtime、避免嵌套"相反；runtime 创建失败会 panic | 直接在 worker 的 async 上下文执行，或明确理由并去掉 `expect` |
| **D-M6** | 🟡 | `BackgroundTaskQueue::new()` 产生非持久化队列，worker 收到任务直接丢弃 | `background_queue.rs:36-41, 197-202` | 公开 API 存在"静默丢任务"路径 | 移除 / 标记 test-only / 强制走 `with_stores` |
| **D-M7** | 🟡 | `.cargo/config.toml` rustflags 写死旧路径 `d:\OpenClaw-Workspace\ShouXin_Complete_v0.2.0` | `.cargo/config.toml` | GPU 构建链接路径失效 | 改相对路径或移除，CUDA 路径走环境变量 |
| **D-M8** | 🟡 | `push_with_persist` 在请求路径同步 `insert + flush` | `background_queue.rs:286-299` | 可能拉高 add 延迟，与"3-6ms"宣称不符 | 评估异步/批量 flush（优化项） |
| **D-M9** | 🟢 | 日志用 `conversation_text.len()`（字节数）当字符数 | `background_queue.rs:214` | 仅日志可读性 | 改 `chars().count()` |

> 另有文档级偏差：`ASYNC_EXTRACT_DESIGN.md §7.3` 仍称"队列不持久化，v0.6.0 才做"，实际已持久化；`PROJECT_INTRO` 的"失败自动重试"则与实现不符。两份文档需在文档治理阶段互相对齐。

---

## 四、本次已修复（Fix #1：提取超时配置化）

**目标**：去掉两处硬编码 `600s`，改为配置驱动，为后续升级留扩展点。

| 文件 | 改动 |
|------|------|
| `src/config/settings.rs` | 新增 `ExtractionSettings { timeout_secs }`（默认 600）；`Settings` 新增 `#[serde(default)] extraction` |
| `src/config/mod.rs` | 导出 `ExtractionSettings` |
| `src/memory/three_library_manager.rs` | 配置新增 `extraction_settings` + `with_extraction_settings()`；同步 `add()` 超时读配置 |
| `src/service/background_queue.rs` | `WorkerState` 新增 `extraction_timeout_secs`；`with_stores` / `spawn_on_runtime` 新增参数；`process_task` 与日志改用配置值 |
| `src/shouxin-server.rs` | 注入 `settings.extraction` 到 manager 与 queue |
| `config.toml` / `config.toml.example` | 新增 `[extraction] timeout_secs = 600` |

**兼容性**：`#[serde(default)]` + 默认值 600，旧配置文件无需修改即可启动。

**复核**：字段、两处 `WorkerState` 构造、唯一调用点（server:458）、builder 链均已人工核对一致。

---

## 五、修复记录与待办

### 已完成

**✅ D-M1：入队失败标记 failed（代码，已编译通过）**
- `three_library_manager.rs::add_async`
  - `push_with_persist` 失败时：记录 `error!`，并 `update_conversation_status(id, "failed", Some("enqueue failed: ..."))`
  - `background_queue` 未配置时：同样标记 `failed`（原为仅 `warn!`）
  - 入队成功日志的长度由字节数改为 `chars().count()`
- 不改 `AddConversationResponse` 契约；客户端可通过 `GET /api/memory/extract_status/:id` 看到失败与原因。

**✅ D-M2：不实现自动重试，修正文档（文档）**
- `PROJECT_INTRO.md`：4.5 节"失败自动重试"改为"失败记录 + 标记 failed（当前不自动重试）"；架构图"失败重试"改为"失败记录"。
- `ASYNC_EXTRACT_DESIGN.md` §7.3：删除过期的"队列不持久化 / v0.6.0 才做"，注明队列已用 sled `pending_extraction_tasks` 持久化并在启动时恢复；把"失败不自动重试"列为当前限制。

### 待修复

**P1**
1. **D-M4** `MAX_ACTIVE_CONTACTS` 收敛为单一配置源
2. **D-M5** worker 执行模型与设计对齐（去掉嵌套 runtime / `expect`）
3. **D-M6** 关闭 `new()` 的静默丢任务路径

**P2**
4. **D-M7** `.cargo/config.toml` 旧路径
5. **D-M8** flush 性能评估
6. **D-M9** 日志字符数（已在 D-M1 顺带修正入队日志）

---

## 六、验证状态

| 项目 | 状态 |
|------|------|
| 静态复核（字段/构造/调用点） | ✅ 通过 |
| `cargo check` 编译验证 | ✅ **两次均通过**：Fix #1 全量 2m18s；D-M1 增量 3.5s；均为 exit 0、32 warnings（原有） |
| 警告 | 32 个，均为原有 dead_code / unused（非本次改动引入），可作为后续清理项 |
| 工具链 | 已用 rustup 安装 stable 到用户目录；crates.io 走本机 `~/.cargo/config.toml` 的 rsproxy 镜像 |
| 预编译产物 | `target/release/shouxin-server.exe`（2026-07-16，196MB），可作为**改动前基线**做冒烟测试，但不包含本次修改 |

**结论**：工具链已就绪，本次改动编译通过；后续功能核对可在此基础上进行（必要时跑集成测试）。

---

## 七、下一步

1. 设计层继续处理 P1：**D-M4**（`MAX_ACTIVE_CONTACTS` 配置收敛）、**D-M5**（worker 执行模型）、**D-M6**（`new()` 静默丢弃）。
2. 设计层收敛后，进入第 1 条链路的**功能核对**（真实调用链 + 状态机 + 三库落点）。
3. 工具链已就绪，可随时用 `cargo check` / `cargo test` 做回归。
