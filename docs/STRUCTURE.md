# ShouXin v0.5.1 项目结构

## 根目录

```
ShouXin_Complete_v0.2.0/
├── Cargo.toml              # 项目配置和依赖
├── Cargo.lock              # 依赖锁定
├── config.toml             # 生产配置（含 gguf）
├── config_hnsw.toml        # HNSW 向量存储配置
├── config_embedded.toml    # 嵌入式向量存储配置
├── src/
│   ├── lib.rs              # 库入口，公共导出
│   ├── main.rs             # 示例程序入口
│   ├── shouxin-server.rs   # 服务器启动入口
│   ├── api/
│   │   ├── mod.rs          # API 模块入口
│   │   ├── routes.rs       # REST API 路由定义
│   │   └── async_memory.rs # AsyncMemory API 包装
│   ├── config/
│   │   ├── mod.rs          # 配置模块入口
│   │   ├── settings.rs     # 全局配置系统 (Settings, LlmSettings, etc.)
│   │   ├── llm.rs          # LLM 配置 (LlmConfig)
│   │   ├── embedder.rs     # 嵌入器配置 (EmbedderConfig)
│   │   └── vector_store.rs # 向量存储配置 (VectorStoreConfig)
│   ├── core/
│   │   ├── mod.rs          # 核心模块入口
│   │   ├── contact.rs      # 核心数据结构 (ContactTotal, ContactIndex, EventIndex, Scope: User/Agent/Contact/Public, OwnerType)
│   │   ├── embedding.rs    # 嵌入相关结构
│   │   ├── fact.rs         # 事实 (Fact) 数据结构
│   │   ├── memory.rs       # 记忆相关结构 (EventDetail, ContactDetail, etc.)
│   │   ├── message.rs      # 消息结构
│   │   └── search_result.rs # 搜索结果结构
│   ├── embedder/
│   │   └── mod.rs          # 嵌入引擎 (FastEmbedder, EmbedderFactory, EmbedderType)
│   ├── error/
│   │   └── mod.rs          # 错误类型 (ShouxinError)
│   ├── heartbeat/
│   │   ├── mod.rs          # 心跳模块入口
│   │   ├── types.rs        # 心跳类型定义 (HeartbeatReport { maintenance_only }, HeartbeatAction, Question, ContactProfile, ProcessingState, EventMergeCandidate, ContactMergeCandidate, AmbiguityInfo)
│   │   ├── executor.rs     # 心跳执行器 (HeartbeatExecutor, 维护/LLM 分离)
│   │   ├── scheduler.rs    # 心跳调度器 (HeartbeatScheduler, 超时控制)
│   │   ├── analyzer.rs     # 心跳分析器 (HeartbeatAnalyzer)
│   │   └── question_pool.rs # 问题池 (QuestionPool)
│   ├── initialization/
│   │   └── mod.rs          # 系统初始化 (SystemInitializer, SystemConfig)
│   ├── learning/
│   │   ├── mod.rs          # 学习模块入口
│   │   ├── entity_kind_classifier.rs # 死代码（未编译，待删除）
│   │   ├── entity_normalizer.rs      # 实体规范化 (EntityNormalizer, EntityAlias)
│   │   ├── knowledge_graph.rs        # 知识图谱提取 (KnowledgeGraphExtractor, KnowledgeTriple)
│   │   ├── learning_logger.rs        # 学习日志 (LearningLogger, LogType)
│   │   ├── slice_relations.rs        # 关系构建 (SliceRelationsBuilder)
│   │   └── summarizer.rs             # 摘要生成 (Summarizer)
│   ├── llm/
│   │   ├── mod.rs          # LLM 模块入口 (LlmClientManager, LlmSourceType)
│   │   ├── api.rs          # API 客户端 (ApiClient)
│   │   ├── factory.rs      # LLM 工厂 (build_llm_manager, build_ollama_client)
│   │   ├── gguf.rs         # GGUF 客户端 (GgufLlmClient)
│   │   ├── manager.rs      # LLM 管理器
│   │   └── ollama.rs       # Ollama 客户端 (OllamaLlmClient)
│   ├── memory/
│   │   ├── mod.rs          # 记忆模块入口
│   │   ├── three_library_manager.rs # 三库记忆管理器 (ThreeLibraryMemoryManager, AddResult)
│   │   └── any_vector_store.rs # 向量存储抽象 (AnyVectorStore)
│   ├── memory_extractor/
│   │   ├── mod.rs          # 提取器模块入口
│   │   ├── extractor.rs    # 核心提取器 (LlmMemoryExtractor, LlmOutputSanitizer, fallback regex 提取)
│   │   ├── dispatcher.rs   # 数据分发器 (DataDispatcher, 分发到三库)
│   │   └── ollama_extractor.rs # Ollama 提取器实现
│   ├── service/
│   │   ├── mod.rs          # 服务模块入口
│   │   ├── background_queue.rs # 后台提取队列 (BackgroundTaskQueue, ExtractionTask)
│   │   ├── file_processor.rs   # 文件处理器 (FileProcessor)
│   │   └── internal_api.rs     # 内部 API (InternalApiState, AgentRegistration, InternalEventAdd)
│   ├── stores/
│   │   ├── mod.rs          # 存储模块入口
│   │   ├── dictionary/
│   │   │   ├── mod.rs      # 字典存储 (SledDictionaryStore)
│   │   │   └── store.rs    # 字典存储实现
│   │   ├── relational/
│   │   │   ├── mod.rs      # 关系存储入口
│   │   │   ├── store.rs    # 关系存储实现 (RelationalStore, schema init + migrations)
│   │   │   ├── event_index.rs    # 事件索引操作
│   │   │   ├── contact_index.rs  # 联系人索引操作
│   │   │   ├── todo_list.rs      # 待办事项操作 (TodoItem, TodoListStore, archive_stale_todos, insert_reminder_todo)
│   │   │   ├── conversation.rs   # 对话存储操作
│   │   │   ├── file_store.rs     # 文件索引与存储 (FileIndex, FileStore)
│   │   │   ├── virtual_entity.rs # 虚拟实体操作 (VirtualEntity)
│   │   │   ├── maintenance.rs    # 数据库维护
│   │   │   ├── contact_background.rs # 联系人背景信息
│   │   │   └── schema.sql        # 数据库 schema 定义
│   │   └── vector/
│   │       ├── mod.rs      # 向量存储入口
│   │       ├── store.rs    # 向量存储实现 (EmbeddedVectorStore)
│   │       ├── factory.rs  # 向量存储工厂 (VectorStoreFactory, VectorStoreType)
│   │       ├── hnsw_store.rs # HNSW 向量存储 (HnswVectorStore)
│   │       └── types.rs    # 向量类型 (LearningMaterial, VectorSearchResult)
│   ├── traits/
│   │   ├── mod.rs          # Trait 定义入口
│   │   ├── embedder.rs     # 嵌入器 Trait
│   │   ├── llm.rs          # LLM Trait
│   │   ├── store_traits.rs # 存储层统一 Trait (ArchiveRecord, ContactRecord, EventIndex, EventDetail, ContactDetail, ArchiveType, etc.)
│   │   └── vector_store.rs # 向量存储 Trait
│   ├── testing/
│   │   └── mod.rs          # 测试工具
│   └── utils/
│       ├── mod.rs          # 工具函数入口
│       ├── time_parser.rs  # 中文日期解析
│       └── validation.rs   # 输入校验
├── docs/
│   ├── API_REFERENCE.md    # API 参考文档
│   ├── TECHNICAL_DOCUMENTATION.md # 技术文档
│   ├── STRUCTURE.md        # 项目结构（本文件）
│   ├── EXTENSION_GUIDE.md  # 扩展指南
│   ├── HEARTBEAT_V0.5.md   # 心跳模块文档
│   └── HEARTBEAT_CURRENT_STATUS.md # 心跳状态文档
├── migrations/
│   └── v0_5_0_heartbeat_upgrade.sql # 心跳模块升级迁移脚本
├── data/                   # 运行时数据目录
│   └── shouxin.db          # SQLite 数据库
├── sled_data/              # Sled 字典存储
├── temp_uploads/           # 临时文件上传
├── .heartbeat_data/        # 心跳数据
├── .fastembed_cache/       # FastEmbed 模型缓存
└── config.toml             # 配置文件
```

### 死代码文件说明

以下文件存在于源码树中，但未在模块树（`mod.rs`）中声明，因此不参与编译。它们属于历史遗留的死代码，待删除：

- `src/intent/mod.rs` — 意图识别模块（未声明）
- `src/intent/classifier.rs` — 意图分类器（未声明）
- `src/learning/entity_kind_classifier.rs` — 实体类型分类器（未声明）
- `src/memory/queue.rs` — 记忆队列（未声明）
- `src/demo.rs` — 演示代码（未声明）

## 二进制入口

| 二进制 | 路径 | 说明 |
|--------|------|------|
| `shouxin-server` | `src/shouxin-server.rs` | 主服务器启动入口，完整组件初始化与生命周期管理 |
| `shouxin` | `src/main.rs` | 示例程序入口 |

## 编译命令

```bash
# 编译（不带 GGUF 支持）
cargo build --release

# 编译（带 GGUF CUDA 支持）
cargo build --release --features gguf

# 运行服务器
cargo run --release --features gguf --bin shouxin-server
```

## 存储架构说明

| 存储层 | 技术 | 用途 | 数据 |
|--------|------|------|------|
| 字典库 | Sled (嵌入式 KV) | 永久全量存档 | ContactTotal, ArchiveRecord |
| 关系库 | SQLite (WAL) | 轻量快速索引 | ContactIndex, EventIndex, TodoItem, Conversation, FileRecord |
| 向量库 | HNSW / Embedded | 语义搜索 | Embedding (768-dim) |

## 数据流

```
用户输入 (对话文本 / 文件上传)
  → POST /api/memory/add
  → ThreeLibraryMemoryManager.add()
  → BackgroundTaskQueue.enqueue()
  → FileProcessor.process()          ← 文件预处理（如适用）
  → LlmMemoryExtractor.extract()     ← LLM 提取 + LlmOutputSanitizer 清洗
  → DataDispatcher.dispatch()        ← 分发到三库
  → 字典库 (永久存档, archive-on-write)
    + 关系库 (索引 + TodoListStore 提醒生成 + archive_stale_todos 清理)
    + 向量库 (语义嵌入)
```

### 心跳数据流

```
HeartbeatScheduler (超时控制)
  → HeartbeatExecutor
    ├── maintenance_only 通道 (数据库维护)
    └── LLM 通道 (事件/联系人合并候选检测, 消歧, 问题生成)
  → HeartbeatAnalyzer (结果分析)
  → QuestionPool (问题队列管理)
```

## v0.5.1 主要变更

- **心跳模块重构**: 引入 `maintenance_only` 分离机制，日常维护与 LLM 分析分别调度；新增 `HeartbeatAction`、`ProcessingState`、合并候选与消歧结构体；类型定义集中到 `types.rs`
- **记忆提取增强**: `LlmOutputSanitizer` 清洗 LLM 原始输出；fallback regex 提取兜底；`DataDispatcher` 明确分发到三库
- **关系库扩展**: 新增 `file_store`、`virtual_entity`、`maintenance`、`contact_background` 子模块；`TodoListStore` 支持 `archive_stale_todos` 和 `insert_reminder_todo`
- **核心模块拆分**: `core/` 下新增 `embedding.rs`、`fact.rs`、`message.rs`、`search_result.rs`，职责更清晰
- **Trait 拆分**: `traits/` 下新增 `embedder.rs`、`llm.rs`、`vector_store.rs`，接口更细粒度
- **工具函数扩展**: 新增 `time_parser.rs`（中文日期解析）和 `validation.rs`（输入校验）
- **服务层扩展**: 新增 `FileProcessor` 文件预处理服务
- **LLM 模块整理**: 文件重命名精简（`gguf_client.rs` → `gguf.rs` 等），新增 `manager.rs`
- **配置文件**: 新增 `config_hnsw.toml` 和 `config_embedded.toml` 示例配置
- **数据库迁移**: 新增 `migrations/v0_5_0_heartbeat_upgrade.sql`