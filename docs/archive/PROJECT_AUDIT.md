# 守心（ShouXin）项目审计清单 · PROJECT_AUDIT

> 审计日期：2026-10-27
> 审计范围：`E:/ShouXin` 工作区全部文档、配置、`src/`、`tests/`、`docs/`、`migrations/`
> 权威版本（用户确认）：**v0.5.0.3**
> 状态说明：本文档只"标问题"，不修改原文件。修复 / 优化 / 升级在后续阶段进行。

---

## 严重度图例

| 标记 | 含义 |
|------|------|
| 🔴 高 | 影响代码运行、测试执行或造成严重误解，应优先处理 |
| 🟡 中 | 文档误导、链接失效、整洁度问题，需修复但非阻塞 |
| 🟢 低 | 历史残留、注释陈旧，可顺手清理 |

---

## 一、版本号混乱（最优先）

项目里同时存在 **至少 5 个互不相同的版本号**，没有任何单一权威来源。

| 编号 | 位置 | 声明版本 | 说明 |
|------|------|----------|------|
| V1 🔴 | `Cargo.toml:3` | `0.5.0` | 与用户确认的 **0.5.0.3** 不一致，应统一 |
| V2 🔴 | `ShouXin_Technical_Document_v4.0.0.md:4` | `v0.3.0` | 技术文档自称"稳定版本"，实际落后约 2 个小版本 |
| V3 🔴 | `docs/STRUCTURE.md:1` 及全部 `FIX_*`/`*_TEST_REPORT.md`/`HEARTBEAT_*`/`SECURITY_AUDIT_REPORT.md` | `v0.5.1` | 多数修复/测试报告围绕 0.5.1 |
| V4 🔴 | `docs/FEATURE_MODULES_CN.md:1` | `v0.5.3` | 又一处不同的版本号 |
| V5 🟡 | `PROJECT_INTRO.md` | `v0.5.0` | 最贴近现状，但版本号仍偏低 |

**建议（V-修复）**：以 `0.5.0.3` 为唯一权威版本，统一 Cargo.toml、所有文档头、迁移脚本版本标记（`store.rs` 内 `'v0.5.1'` 等历史标记保留作迁移记录，但补充当前版本注释规范）。建立简单的 CHANGELOG 或版本常量，避免再次漂移。

---

## 二、文档准确性问题（按文档）

### D1 🔴 `ShouXin_Technical_Document_v4.0.0.md`（技术文档）
整体严重过时，建议重写或降格为历史归档：
1. 自称"项目版本 v0.3.0 / 稳定 / 功能完整"，实际代码已是 0.5.x。
2. 技术栈表**只列 Ollama 一个 LLM 后端**，且贴出的 `[dependencies]` 片段缺少 `llama-cpp-2`、`constant_time_eq`、`backoff`、`sysinfo`、`pdf-extract`、`zip`、`lru`、`encoding_rs`、`bytes` 等大量真实依赖——描述的是旧版依赖。
3. 写"**四要素**提取（联系人/事件/对话文本/学习素材）"，实际为**五类**（人物/事件/待办/事实/学习素材）。
4. 目录树把 `intent/classifier.rs`、`learning/entity_kind_classifier.rs` 列为正常编译模块，实则从未被 `mod` 声明（死代码），且文件现已删除。
5. 文档内含大量 `file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/...` 失效链接。

### D2 🟡 `docs/STRUCTURE.md`（标注 v0.5.1，大体准确但有偏差）
1. "死代码"小节列出的 `src/intent/mod.rs`、`src/intent/classifier.rs`、`src/learning/entity_kind_classifier.rs`、`src/memory/queue.rs`、`src/demo.rs` **实际已从磁盘删除**（仅留空 `intent/` 目录）。描述应改为"已清理"。
2. `embedder` 只写 `mod.rs`，实际有 `fastembed.rs` / `factory.rs` / `mock.rs`。
3. `initialization` 写成 `initialization/mod.rs`，实际是单文件 `initialization.rs`；`error` 同理是 `error.rs`。
4. `migrations/` 只列 1 个 SQL，实际有 **5 个**：`20260528_add_owner_fields.sql`、`v0_2_0_to_v0_3_0.sql`、`v0_3_0_add_files.sql`、`v0_5_0_heartbeat_upgrade.sql`、`v0_5_1_event_semantic_merge.sql`。
5. 根目录名仍写 `ShouXin_Complete_v0.2.0/`（第 6 行），陈旧。

### D3 🟢 `docs/TECHNICAL_DOCUMENTATION.md`(42KB) / `docs/API_REFERENCE.md`(51KB) / `docs/FEATURE_MODULES.md`(英文)
内容体量大，**版本声明与链接需纳入统一核对待修订**（本次未逐行通读，列为待核对项）。

### D4 🟢 `PROJECT_INTRO.md`（最贴近现状）
三后端、五要素、心跳、热冷分离、后台队列均正确。小缺口：未说明 `intent/` 空目录、`[agent]` 内部 API（`service/internal_api.rs` 真实存在）、`testing/` 测试辅助框架；版本号应改 0.5.0.3。

---

## 三、工程 / 代码实际问题

### C1 🔴 `tests/common.rs` 硬编码旧工作区绝对路径（真实缺陷）
- 第 28 行、第 83 行：`std::path::PathBuf::from("D:\\OpenClaw-Workspace\\ShouXin_Complete_v0.2.0\\.fastembed_cache")`
- 并据此设置 `HF_HUB_CACHE` / `FASTEMBED_CACHE_DIR` 等环境变量。
- **后果**：当前工作区为 `E:\ShouXin`，该旧路径不存在；FastEmbed 在离线模式（`HF_HUB_OFFLINE=1`）下会找不到嵌入缓存，导致文件相关 Rust 集成测试（`tests/test_file_upload.rs` 等）初始化失败。
- **建议**：改为仓库相对路径解析（如基于 `CARGO_MANIFEST_DIR`/环境变量），或读取真正的 `.fastembed_cache` 位置，使 `cargo test` 在当前机器可跑。

### C2 🟡 大量文档含失效绝对链接 `file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/...`
涉及：`FINAL_V0.5.1_ENHANCED_FIXES.md`、`V0.5.1_TASKS_5-8_TEST_SUMMARY.md`、`TODO_TIMEOUT_DIAGNOSIS.md`、`SECURITY_AUDIT_REPORT.md`、`EVENT_*`/`TODO_*`/`*_HOT_LIMIT*` 等报告。
- **建议**：批量替换为相对路径或仓库根相对链接（可脚本化）。

### C3 🟢 `tests/SECURITY_FIX_VERIFICATION_REPORT.md:30` 引用旧绝对路径。

### C4 🟢 `tests/test_file_upload.rs:15` 注释写 "v0.3.0 新增的多媒体文件文本化存储功能"，版本陈旧。

### C5 🟢 `src/intent/` 为空目录
文件已删，仅留目录壳。应删除空目录；若后续升级计划重做"意图识别"模块，则补回模块并在 `lib.rs` 声明。

### C6 🟡 安全审计遗留项复核
`docs/SECURITY_AUDIT_REPORT.md` 指出曾将明文 admin token `shouxin-v050-gguf-test-token` 提交版本控制。已确认 `config.toml` 中 `internal_api_token = ""` 改为走环境变量（`SHOUXIN_*_INTERNAL_API_TOKEN`），方向正确。但需**复核代码中是否还有其它硬编码 token / 密钥残留**。

---

## 四、后续"修复 + 优化 + 升级"路线图（待规划，非本次执行）

| 编号 | 方向 | 候选动作 |
|------|------|----------|
| U1 | 版本与发布治理 | 统一版本号、建立 CHANGELOG、迁移脚本版本标记规范化 |
| U2 | 文档体系重构 | 单一权威 ARCHITECTURE + API 文档；链接 lint；废弃重复文档 |
| U3 | 测试工程化 | 去掉硬编码路径（C1）、CI 友好、`cargo test` 全量可跑 |
| U4 | 清理残留 | 删除空 `intent/` 目录、复核未使用导出、清理失效链接 |
| U5 | 依赖与体积 | 评估 `llama-cpp-2`(0.1.146 锁定)、`fastembed`(4.9.1) 升级；`gguf-cuda` 默认策略 |
| U6 | 安全加固 | token 管理复核、配置隔离、依赖安全审计 |
| U7 | 性能 / 可观测性 | 心跳自适应调度、tracing 完善、以现有压测报告为基线 |
| U8 | 功能升级 | 多用户 / Agent 作用域深化（代码中已部分支持 scope/owner） |

---

## 五、本次核查结论

- **代码现状健康**：`Cargo.toml` 技术栈、`config.toml` 配置、`src/lib.rs` 模块树三者自洽；三库 / 提取 / 心跳 / 多 LLM 后端架构完整落地。
- **最大问题是"文档与版本漂移"**：版本号 5 处不一致、技术文档停留在 v0.3.0、大量失效链接、测试硬编码旧路径。
- **唯一会实际阻断测试运行的真缺陷是 C1**（测试缓存路径）。
- 建议以本清单为作战地图，后续按 V（版本）→ C1/C2（运行阻断）→ D（文档）→ U（升级）顺序推进。

---

## 六、补充核查（第二轮：docs 全目录）

> 第二轮已通读 `docs/` 全部 9 份文档，新发现如下（详细清单见 `MODULE_INVENTORY.md` 第七节）。

| 编号 | 严重度 | 问题 |
|------|--------|------|
| N1 | 🟡 | `docs/TECHNICAL_DOCUMENTATION.md §4` 模块结构树不准确：列了不存在文件 `core/event.rs`、`learning/event_dedup.rs`、`llm/sanitizer.rs`、`memory/archive_manager.rs`；`error/error.rs`、`initialization/initialization.rs` 路径形状错误；漏列 `background_queue.rs`、`learning_logger.rs`、`slice_relations.rs`、`summarizer.rs`、`any_vector_store.rs`、`ollama_extractor.rs`、`core/{fact,message,search_result}.rs`、`utils/{time_parser,validation}.rs` |
| N2 | 🟢 | `TECHNICAL_DOCUMENTATION.md §10.2` 写 17 张表，实际 18 张，漏 `event_similarity_cache` |
| N3 | 🟡 | `API_REFERENCE.md`（v0.5.3）覆盖 33 端点，漏 Internal API 2 个：`/internal/agent/register`、`/internal/event/add` |
| N4 | 🟡 | 功能模块文档中英文版本分叉：`FEATURE_MODULES_CN.md` = v0.5.3，`FEATURE_MODULES.md` = v0.5.1 |
| N5 | 🟡 | Trait 定义冗余/重复：`DictionaryStoreTrait` vs `DictionaryStore`；`RelationalStoreTrait` 在 `traits/store_traits.rs` 与 `stores/relational/mod.rs` 各定义一次 |
| N6 | 🟡 | 心跳/安全/扩展三份文档仍停留 v0.5.1，未跟进 v0.5.2/v0.5.3 的修复（ExecutingGuard、孤立向量清理、记忆生命周期、事件去重三重防护、LLM 重要性传递） |
| N7 | 🟡 | 版本口径不一：用户确认 0.5.0.3，最新文档用 v0.5.3，`Cargo.toml` 仍是 0.5.0，疑似同一版本的不同写法，需统一 |

**第二轮结论**：`docs/TECHNICAL_DOCUMENTATION.md` 才是当前最新、最完整的技术文档（已记录 v0.5.2/v0.5.3），而根目录 `ShouXin_Technical_Document_v4.0.0.md` 是历史残留（v0.3.0）。后续文档治理应"以 TECHNICAL_DOCUMENTATION + API_REFERENCE + FEATURE_MODULES_CN 为三份权威，其余归档或合并"。

---

## 七、第三轮：记忆链路功能核对新增缺陷

> 详见 `FIX_MEMORY_CHAIN_FUNCTION.md`。

| 编号 | 严重度 | 问题 | 证据 |
|------|--------|------|------|
| F-M1 | 🔴 | 异步写回用 `ContactRecord` 覆盖字典库 `ContactTotal`，aliases / background / notes / tags / related / weight 被清空 | `background_queue.rs:376-391`、`store.rs:743-774` |
| F-M2 | 🔴 | 异步路径未写 `contact_event_history`，字典库缺"联系人事件历史" | `dispatcher.rs:193-221`；`background_queue.rs` 无 `save_contact_event` |
| F-M3 | 🔴 | `ContactRecord` 无 `owner_type`，trait 实现硬编码 `OwnerType::User`，Agent 作用域数据被写成 User | `store.rs:758` |
| F-M4 | 🟠 | 异步写三库无回滚/事务，失败留部分写入 | `background_queue.rs::write_extraction_results` |
| F-M5 | 🟠 | 同步 `DataDispatcher` 与异步 `write_extraction_results` 两套写路径重复实现 | 架构债（F-M1~F-M4 根因） |
| F-M6 | 🟡 | `completed` 状态写回错误被 `let _ =` 忽略 | `background_queue.rs:279` |

**结论**：默认异步链路高层流程可用，但字典库写入有损、Agent 隔离被破坏。建议优先级高于文档问题。
