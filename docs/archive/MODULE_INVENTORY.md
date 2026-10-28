# 守心（ShouXin）模块 · 组件 · 功能 清单

> 生成日期：2026-10-27
> 依据：`docs/` 全部 9 份文档 + `src/` 实际代码 + `migrations/` + `src/api/routes.rs` + `src/shouxin-server.rs`
> 版本口径：用户确认 **0.5.0.3**；文档侧最新使用 **v0.5.3**（详见第八节"版本说明"）
> 用途：作为后续"逐项核对功能实现情况"的基线清单。状态列初始均为 ⬜ 待核对。

---

## 一、总量统计（实测）

| 维度 | 数量 | 来源 / 说明 |
|------|------|-------------|
| 顶层代码模块 | **16** | `src/lib.rs` 的 `pub mod` 声明 |
| 源文件（.rs） | **87** | `find src -name '*.rs'` |
| 源码总行数 | **28,760** | 实测 |
| 功能模块（文档口径） | **14** | `docs/FEATURE_MODULES_CN.md` |
| HTTP 端点（实际） | **35** | 主路由 31 + Admin 2 + Internal 2 |
| HTTP 端点（文档覆盖） | 33 | `API_REFERENCE.md`，**漏 Internal 2 个** |
| 存储库 | **3** | 字典库 Sled / 关系库 SQLite / 向量库 HNSW·Embedded |
| LLM 后端 | **3** | GGUF / Ollama / OpenAI 兼容 API |
| 数据表 | **18** | 13 张 .sql + 5 张代码内建表（文档写 17，漏 1） |
| `pub struct` | ≈ **179** | 含大量 API DTO |
| `pub enum` | ≈ **30** | — |
| `pub trait` | **10** | 存在命名重复（见 N5） |
| docs 文档 | **9** | `docs/*.md` |
| 根目录报告 | **29** | `*.md`（FIX_*/TEST_REPORT 等） |
| Rust 集成测试 | **9** | `tests/*.rs` |
| Python 测试脚本 | **9** | `tests/*.py` |
| 示例 | **7** | `examples/*.rs` |
| 迁移脚本 | **5** | `migrations/*.sql` |

---

## 二、16 个顶层代码模块

| # | 模块 | 职责 | 主要文件 | 状态 |
|---|------|------|----------|------|
| 1 | `api` | REST 路由与同步/异步封装 | routes.rs(63KB)、async_memory.rs、sync.rs | ⬜ |
| 2 | `config` | 多层配置系统 | settings.rs、llm.rs、embedder.rs、vector_store.rs、app.rs | ⬜ |
| 3 | `core` | 核心数据结构 | contact.rs、embedding.rs、fact.rs、memory.rs、message.rs、search_result.rs | ⬜ |
| 4 | `embedder` | 嵌入引擎 | fastembed.rs、factory.rs、mock.rs | ⬜ |
| 5 | `error` | 统一错误 | error.rs | ⬜ |
| 6 | `heartbeat` | 心跳调度/执行/分析/问题池 | scheduler、executor、analyzer、question_pool、types | ⬜ |
| 7 | `initialization` | 系统初始化 | initialization.rs | ⬜ |
| 8 | `learning` | 学习与知识 | entity_normalizer、knowledge_graph、learning_logger、slice_relations、summarizer | ⬜ |
| 9 | `llm` | 多后端 LLM | gguf、ollama、api、manager、factory | ⬜ |
| 10 | `memory` | 三库记忆管理 | three_library_manager.rs(44KB)、any_vector_store.rs | ⬜ |
| 11 | `memory_extractor` | LLM 记忆提取 | extractor.rs、dispatcher.rs、ollama_extractor.rs | ⬜ |
| 12 | `service` | 后台服务 | background_queue.rs(40KB)、file_processor.rs、internal_api.rs | ⬜ |
| 13 | `stores` | 三库存储实现 | dictionary/、relational/、vector/ | ⬜ |
| 14 | `testing` | 测试辅助 | assertions、config、recorder、utils | ⬜ |
| 15 | `traits` | 可扩展接口 | llm、embedder、vector_store、store_traits | ⬜ |
| 16 | `utils` | 工具 | time_parser（中文日期）、validation | ⬜ |

> 注：`src/intent/` 为空目录（未在 lib.rs 声明），不计入。

---

## 三、14 个功能模块（文档口径 → 代码落点）

| # | 功能模块 | 端点数 | 代码落点 | 状态 |
|---|----------|--------|----------|------|
| 1 | 记忆管理 | 6 | `api/`、`memory/three_library_manager.rs`、`memory_extractor/` | ⬜ |
| 2 | 待办管理 | 5 | `stores/relational/todo_list.rs`、`api/routes.rs` | ⬜ |
| 3 | 心跳系统 | 4 | `heartbeat/` 全模块 | ⬜ |
| 4 | 文件管理 | 4 | `service/file_processor.rs`、`stores/relational/file_store.rs` | ⬜ |
| 5 | 虚拟实体管理 | 6 | `stores/relational/virtual_entity.rs` | ⬜ |
| 6 | 事件合并管理 | 2 | `stores/relational/event_index.rs`、`heartbeat/` | ⬜ |
| 7 | 向量搜索 | 2 | `stores/vector/` | ⬜ |
| 8 | 智能搜索 | 1 | `api/routes.rs::smart_search` | ⬜ |
| 9 | 内部 API | 2 | `service/internal_api.rs` | ⬜ |
| 10 | 管理 API | 2 | `shouxin-server.rs`（Admin 路由 + Token） | ⬜ |
| 11 | 健康检查 | 1 | `api/routes.rs::health_check` | ⬜ |
| 12 | LLM 系统 | 0（内部） | `llm/`、`traits/llm.rs` | ⬜ |
| 13 | 三库存储 | 0（内部） | `stores/` | ⬜ |
| 14 | 记忆提取 | 0（内部） | `memory_extractor/` | ⬜ |

---

## 四、关键组件清单

### 4.1 存储组件
| 组件 | 位置 | 说明 | 状态 |
|------|------|------|------|
| `SledDictionaryStore` | `stores/dictionary/store.rs` | 字典库（永久存档） | ⬜ |
| `RelationalStore` | `stores/relational/store.rs` | 关系库（SQLite 索引 + 迁移） | ⬜ |
| `EmbeddedVectorStore` | `stores/vector/store.rs` | 暴力搜索向量库 | ⬜ |
| `HnswVectorStore` | `stores/vector/hnsw_store.rs` | HNSW 近似检索 | ⬜ |
| `VectorStoreFactory` | `stores/vector/factory.rs` | 向量库工厂（双模式） | ⬜ |
| `AnyVectorStore` | `memory/any_vector_store.rs` | 向量库运行时抽象 | ⬜ |

### 4.2 记忆与提取组件
| 组件 | 位置 | 说明 | 状态 |
|------|------|------|------|
| `ThreeLibraryMemoryManager` | `memory/three_library_manager.rs` | 三库统一管理 | ⬜ |
| `LlmMemoryExtractor` | `memory_extractor/extractor.rs` | LLM 提取主实现 | ⬜ |
| `SimpleExtractor` | `memory_extractor/extractor.rs` | 简单/兜底提取 | ⬜ |
| `LlmOutputSanitizer` | `memory_extractor/extractor.rs` | LLM 乱码/JSON 清洗 | ⬜ |
| `DataDispatcher` | `memory_extractor/dispatcher.rs` | 三库分流 | ⬜ |
| `OllamaExtractor` | `memory_extractor/ollama_extractor.rs` | Ollama 专用提取 | ⬜ |

### 4.3 心跳组件
| 组件 | 位置 | 说明 | 状态 |
|------|------|------|------|
| `HeartbeatScheduler` | `heartbeat/scheduler.rs` | 周期调度 + 超时/内存保护 | ⬜ |
| `HeartbeatExecutor` | `heartbeat/executor.rs` | 分阶段执行（维护 / LLM） | ⬜ |
| `HeartbeatAnalyzer` | `heartbeat/analyzer.rs` | LLM 结果分析 | ⬜ |
| `QuestionPool` | `heartbeat/question_pool.rs` | 待确认问题池 | ⬜ |
| `HeartbeatReport` / `HeartbeatAction` / `ProcessingState` / `ExecutingGuard` | `heartbeat/types.rs` | 报告与状态类型 | ⬜ |

### 4.4 LLM 组件
| 组件 | 位置 | 说明 | 状态 |
|------|------|------|------|
| `LlmClientManager` | `llm/manager.rs` | 主/备切换 + 健康检查 | ⬜ |
| `GgufLlmClient` | `llm/gguf.rs` | 本地 llama.cpp | ⬜ |
| `OllamaLlmClient` | `llm/ollama.rs` | Ollama API | ⬜ |
| `ApiClient` | `llm/api.rs` | OpenAI 兼容 | ⬜ |
| `build_llm_manager` | `llm/factory.rs` | 工厂装配 | ⬜ |

### 4.5 服务 / 学习 / 工具组件
| 组件 | 位置 | 说明 | 状态 |
|------|------|------|------|
| `BackgroundTaskQueue` | `service/background_queue.rs` | Sled 持久化异步队列 | ⬜ |
| `FileProcessor` | `service/file_processor.rs` | txt/pdf/zip 文本化 | ⬜ |
| `InternalApiState` / `AgentRegistration` | `service/internal_api.rs` | Agent 注册与内部事件 | ⬜ |
| `EntityNormalizer` | `learning/entity_normalizer.rs` | 实体标准化 | ⬜ |
| `KnowledgeGraphExtractor` | `learning/knowledge_graph.rs` | 知识三元组 | ⬜ |
| `SliceRelationsBuilder` | `learning/slice_relations.rs` | 切片关系 | ⬜ |
| `Summarizer` | `learning/summarizer.rs` | 摘要 | ⬜ |
| `LearningLogger` | `learning/learning_logger.rs` | 学习日志 | ⬜ |
| `time_parser` / `validation` | `utils/` | 中文日期解析 / 输入校验 | ⬜ |

---

## 五、HTTP 端点全量（35）

### 5.1 主路由（31，`src/api/routes.rs`）
| 方法 | 路径 |
|------|------|
| GET | `/health` |
| POST | `/api/memory/add` |
| GET | `/api/memory/extract_status/:conversation_id` |
| POST | `/api/memory/search/contacts` |
| POST | `/api/memory/search/events` |
| POST | `/api/memory/search/history` |
| POST | `/api/memory/search/semantic` |
| POST | `/api/events/pending_merges` |
| POST | `/api/events/confirm_merge` |
| POST | `/api/vector/search` |
| GET | `/api/vector/:id` |
| POST | `/api/todos` |
| GET | `/api/todos` |
| GET | `/api/todos/:id` |
| PUT | `/api/todos/:id/complete` |
| DELETE | `/api/todos/:id` |
| POST | `/api/heartbeat/trigger` |
| GET | `/api/heartbeat/pending_questions` |
| GET | `/api/heartbeat/status` |
| POST | `/api/heartbeat/answer` |
| POST | `/api/search/smart` |
| GET | `/api/virtual_entities` |
| GET | `/api/virtual_entities/:id` |
| POST | `/api/virtual_entities` |
| PUT | `/api/virtual_entities/:id` |
| DELETE | `/api/virtual_entities/:id` |
| GET | `/api/virtual_entities/check/:name` |
| POST | `/api/files/upload` |
| GET | `/api/files/:file_id` |
| GET | `/api/files/:file_id/text` |
| DELETE | `/api/files/:file_id` |

### 5.2 Admin（2，`src/shouxin-server.rs`，需 Token）
| 方法 | 路径 |
|------|------|
| GET | `/admin/llm/status` |
| POST | `/admin/llm/switch` |

### 5.3 Internal（2，`src/service/internal_api.rs`）
| 方法 | 路径 |
|------|------|
| POST | `/internal/agent/register` |
| POST | `/internal/event/add` |

---

## 六、数据表（18 张实测）

| # | 表名 | 建表位置 | 文档是否列出 |
|---|------|----------|--------------|
| 1 | `contact_index` | schema.sql | ✅ |
| 2 | `event_index` | schema.sql | ✅ |
| 3 | `todo_list` | schema.sql | ✅ |
| 4 | `file_index` | v0_3_0_add_files.sql | ✅ |
| 5 | `file_relations` | v0_3_0_add_files.sql | ✅ |
| 6 | `pending_event_merge` | v0_5_1_event_semantic_merge.sql | ✅ |
| 7 | `event_similarity_cache` | v0_5_1_event_semantic_merge.sql | ❌ **漏列** |
| 8 | `memory_processing_state` | v0_5_0_heartbeat_upgrade.sql | ✅ |
| 9 | `contact_profile_update_log` | v0_5_0_heartbeat_upgrade.sql | ✅ |
| 10 | `contact_merge_log` | v0_5_0_heartbeat_upgrade.sql | ✅ |
| 11 | `event_merge_log` | v0_5_0_heartbeat_upgrade.sql | ✅ |
| 12 | `ambiguity_log` | v0_5_0_heartbeat_upgrade.sql | ✅ |
| 13 | `contact_alias_index` | v0_5_0_heartbeat_upgrade.sql | ✅ |
| 14 | `entity_alias` | store.rs 内建 | ✅ |
| 15 | `slice_relation` | store.rs 内建 | ✅ |
| 16 | `slice_keyword` | store.rs 内建 | ✅ |
| 17 | `schema_migrations` | store.rs 内建 | ✅ |
| 18 | `virtual_character_aliases` | store.rs 内建 | ✅ |

---

## 七、docs 全目录核查结论（逐份）

| 文档 | 自称版本 | 核查结论 |
|------|----------|----------|
| `API_REFERENCE.md` | **v0.5.3** | 33 端点，覆盖主路由+Admin，**漏 Internal 2 个**（N3） |
| `TECHNICAL_DOCUMENTATION.md` | **v0.5.3**（最新） | 内容最新，但 §4 模块树列了**不存在文件**、漏了真实文件（N1）；§10 表漏 1 张（N2） |
| `FEATURE_MODULES_CN.md` | **v0.5.3** | 14 模块齐全；与英文版版本不同步（N4） |
| `FEATURE_MODULES.md`（英文） | v0.5.1 | 落后于中文版；14 模块结构一致 |
| `STRUCTURE.md` | v0.5.1 | 死代码清单已过时；embedder/initialization/error 路径描述偏差；migrations 漏 4 个（D2） |
| `HEARTBEAT_V0.5.md` | v0.5.1 | 心跳拆分架构描述完整，未跟进 v0.5.2/v0.5.3 |
| `HEARTBEAT_CURRENT_STATUS.md` | v0.5.1 | 状态快照，同样未更新 |
| `SECURITY_AUDIT_REPORT.md` | v0.5.1 | 11 项审计（5 已修复/6 待办），需复核当前代码 |
| `EXTENSION_GUIDE.md` | v0.5.1 | 扩展指引有效，版本偏旧 |
| `ShouXin_Technical_Document_v4.0.0.md`（根目录） | **v0.3.0** | 严重过时，建议重写或归档（D1） |

### 本轮新增问题（N 系列）
- **N1 🟡** `TECHNICAL_DOCUMENTATION.md §4` 模块结构不准确：
  - 列了不存在：`core/event.rs`、`learning/event_dedup.rs`、`llm/sanitizer.rs`、`memory/archive_manager.rs`
  - 路径形状错误：`error/error.rs` → 实为 `error.rs`；`initialization/initialization.rs` → 实为 `initialization.rs`
  - 漏列真实：`service/background_queue.rs`、`learning/{learning_logger,slice_relations,summarizer}.rs`、`memory/any_vector_store.rs`、`memory_extractor/ollama_extractor.rs`、`core/{fact,message,search_result}.rs`、`utils/{time_parser,validation}.rs`
- **N2 🟢** `§10.2` 数据表 17 张，实际 18 张（漏 `event_similarity_cache`）。
- **N3 🟡** `API_REFERENCE.md` 未收录 Internal API 2 个端点。
- **N4 🟡** 中英文功能模块文档版本不同步（CN v0.5.3 / EN v0.5.1），内容可能已分叉。
- **N5 🟡** Trait 定义存在命名重复/并行：`DictionaryStoreTrait`(traits) 与 `DictionaryStore`(stores/dictionary/mod.rs)、`RelationalStoreTrait` 在 `traits/store_traits.rs` 与 `stores/relational/mod.rs` 各定义一次。属设计冗余，后续优化项。
- **N6 🟡** 心跳/安全/扩展三份文档仍停留在 v0.5.1，未跟进 v0.5.2/v0.5.3 的修复（ExecutingGuard、孤立向量清理、记忆生命周期、事件去重三重防护、LLM 重要性传递）。

---

## 八、版本号口径说明

- 用户确认当前版本：**0.5.0.3**
- 最新文档使用：**v0.5.3**（`API_REFERENCE.md`、`FEATURE_MODULES_CN.md`、`TECHNICAL_DOCUMENTATION.md`）
- 判断：二者**极可能指同一版本**（4 段式 `0.5.0.3` vs 3 段式 `0.5.3`），但命名必须统一。
- `Cargo.toml` 仍为 `0.5.0`，需在版本治理阶段统一。
- 其余文档停留在 v0.5.1，个别停留在 v0.3.0。

---

## 九、后续"功能实现情况核查"清单（下一阶段）

按以下顺序逐项核对"文档宣称 → 代码是否真实实现 → 是否有测试覆盖"：

1. **记忆管理链路**：`/api/memory/add` → 队列 → 提取 → 分发 → 三库写入 → 状态查询
2. **待办全生命周期**：CRUD + 去重 + 热冷归档 + 上限提醒 + deadline 失效
3. **心跳双阶段**：maintenance_only 与 LLM 阶段是否真正分离；ExecutingGuard；孤立向量清理
4. **事件生命周期**：自动失效 `auto_invalid_at`、提升/降级、双路径归档、语义合并去重
5. **文件处理**：上传 → 异步文本化 → 索引 → 检索/删除
6. **虚拟实体**：CRUD + 名称匹配检查 + 别名索引
7. **向量检索**：HNSW 与 Embedded 双模式实际可用性
8. **LLM 多后端**：GGUF / Ollama / API 切换 + 主备故障转移 + 心跳独立后端
9. **三库一致性**：写入失败回滚、重启恢复、跨库一致性
10. **安全项**：Token 常量时间比较、路径校验、速率限制（是否仍未实现）

> 每一项核对完成后，将本文件对应行的状态从 ⬜ 改为 ✅ 已实现 / ⚠️ 部分实现 / ❌ 未实现。
