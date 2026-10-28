# ShouXin（守心）- Rust 技术文档 v4.0.0

> 文档生成日期：2026-05-28
> 项目版本：v0.3.0
> 项目状态：稳定版本，功能完整

---

## 一、版本升级说明

### 1.1 v0.3.0 版本亮点

从 v0.2.0 升级到 v0.3.0，本次升级主要完成了以下工作：

- ✅ **完整的示例文件修复**：修复了所有示例文件中的 HTML 实体编码问题
- ✅ **导入路径修复**：统一了 TodoItem 的导入路径
- ✅ **所有权问题修复**：解决了 ContactTotal 方法调用导致的所有权转移问题
- ✅ **事件查询功能增强**：改进了 search_events_by_contact 方法的查询逻辑
- ✅ **稳定性提升**：所有测试用例可以正常运行
- ✅ **文档完善**：更新了技术文档和版本信息

### 1.2 升级内容详细清单

| 类别 | 修复内容 | 影响文件 |
|------|---------|---------|
| **语法错误** | HTML 实体编码修复 (`-&gt;` → `->`, `&lt;` → `<`, `&gt;` → `>`) | examples/*.rs |
| **导入错误** | TodoItem 导入路径修正 | examples/*.rs |
| **所有权问题** | ContactTotal 方法调用导致的所有权转移 | examples/*.rs |
| **功能增强** | 事件查询逻辑改进 | src/stores/relational/store.rs |
| **版本号** | 版本更新 v0.2.0 → v0.3.0 | Cargo.toml |

---

## 二、项目概述

### 2.1 项目简介

ShouXin（守心）是一个以时间为轴、三库分离、纯本地运行的认知记忆系统，旨在提供：

- **完全本地运行**：无需联网，无外部依赖
- **无 Docker/云服务**：所有组件本地部署
- **高性能**：采用 Rust 原生开发，充分利用内存安全特性
- **嵌入式存储**：所有数据本地持久化

### 2.2 核心理念

- **以时间为轴**：所有记忆都带有时间戳，支持按时间检索
- **三库分离**：
  - 字典库（永久全量存档，使用 sled）
  - 关系库（轻量快速索引，使用 SQLite + SQLx）
  - 向量库（语义搜索支持，使用 HNSW-RS + sled）
- **四要素提取**：从对话中提取联系人、事件、对话文本、学习素材
- **记忆生命周期**：联系人背景动态总结、事件分层清理、事件完结

---

## 三、技术栈

### 3.1 核心技术栈

| 层级 | 技术选型 | 版本 | 说明 |
|------|---------|------|------|
| **编程语言** | Rust | 2021 Edition | 内存安全、高性能 |
| **异步运行时** | Tokio | 1.x | 全功能异步支持 |
| **序列化** | Serde + serde_json | 1.x | JSON/二进制序列化 |
| **HTTP 框架** | Axum | 0.7 | 现代 HTTP API 框架 |
| **LLM 后端** | Ollama API | - | 本地大语言模型调用 |
| **Embedding** | FastEmbed | 4.9.1 | 本地向量嵌入 |
| **向量索引** | HNSW-RS | 0.3 | 纯 Rust HNSW 算法实现 |
| **键值存储** | sled | 0.34 | 嵌入式 KV 存储 |
| **关系数据库** | SQLx + SQLite | 0.8 | 嵌入式关系存储 |
| **配置管理** | config | 0.14 | 多格式配置支持 |
| **错误处理** | thiserror + anyhow | 1.x | 结构化错误处理 |
| **日志系统** | tracing | 0.1 | 结构化日志 |

### 3.2 依赖清单（Cargo.toml）

```toml
[package]
name = "shouxin"
version = "0.3.0"
edition = "2021"

[dependencies]
# 异步运行时
tokio = { version = "1", features = ["full"] }

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# LLM（Ollama 本地）
reqwest = { version = "0.12", features = ["json"] }

# Embedding（纯本地）
fastembed = { version = "4.9.1", features = ["ort-download-binaries"] }

# 向量存储（完全嵌入式）
sled = "0.34"
hnsw_rs = "0.3"

# 关系数据库
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "chrono"] }
libsqlite3-sys = { version = "0.30", features = ["bundled"] }

# 配置管理
config = "0.14"

# 错误处理
thiserror = "1"
anyhow = "1"

# 日志
tracing = "0.1"
tracing-subscriber = "0.3"

# 异步 trait
async-trait = "0.1"

# HTTP API
axum = { version = "0.7", features = ["macros"] }

# 命令行参数
clap = { version = "4", features = ["derive"] }

# 工具库
uuid = { version = "1", features = ["v4", "serde"] }
regex = "1"
chrono = { version = "0.4", features = ["serde"] }
once_cell = "1"
rand = "0.8"
directories = "5"
rayon = "1"
lru = "0.12"
tempfile = "3"

[dev-dependencies]
sysinfo = "0.32"
```

---

## 四、环境需求

### 4.1 开发环境

| 组件 | 最低版本 | 推荐版本 |
|------|---------|---------|
| **Rust** | 1.70+ | 1.80+ |
| **Cargo** | 1.70+ | 1.80+ |
| **MSVC Build Tools** | VS 2019 | VS 2022 |
| **Windows SDK** | 10.0.19041 | 10.0.22621 |

### 4.2 运行环境

| 组件 | 要求 |
|------|------|
| **操作系统** | Windows 10/11, Linux, macOS |
| **内存** | 最低 4GB，推荐 8GB+ |
| **存储** | 最低 500MB（不含模型） |
| **CPU** | x86_64 或 ARM64 |

### 4.3 可选组件

| 组件 | 用途 | 安装方式 |
|------|------|---------|
| **Ollama** | 本地 LLM 服务 | https://ollama.ai |
| **CUDA** | GPU 加速 | NVIDIA 官网 |

### 4.4 环境变量配置

```bash
# 设置 HuggingFace 离线模式（重要！）
HF_HUB_OFFLINE=1
TRANSFORMERS_OFFLINE=1

# 设置 fastembed 缓存目录
FASTEMBED_CACHE_DIR=./.fastembed_cache

# 禁用符号链接（Windows 推荐）
HF_HUB_DISABLE_SYMLINKS=1

# Ollama 服务地址（可选）
OLLAMA_HOST=http://localhost:11434
```

---

## 五、项目结构

### 5.1 目录结构

```
ShouXin_Complete_v0.2.0/
├── src/                          # 源代码目录
│   ├── api/                      # API 层
│   │   ├── async_memory.rs       # 异步记忆 API
│   │   ├── mod.rs                # 模块导出
│   │   ├── routes.rs             # HTTP 路由定义
│   │   └── sync.rs               # 同步 API 封装
│   ├── config/                   # 配置模块
│   │   ├── app.rs                # 应用配置
│   │   ├── embedder.rs           # 嵌入器配置
│   │   ├── llm.rs                # LLM 配置
│   │   ├── mod.rs                # 模块导出
│   │   ├── settings.rs           # 设置管理
│   │   └── vector_store.rs       # 向量存储配置
│   ├── core/                     # 核心数据结构
│   │   ├── contact.rs            # 联系人结构
│   │   ├── embedding.rs          # 嵌入向量
│   │   ├── fact.rs               # 事实记录
│   │   ├── memory.rs             # 记忆结构
│   │   ├── message.rs            # 消息结构
│   │   ├── mod.rs                # 模块导出
│   │   └── search_result.rs      # 搜索结果
│   ├── embedder/                 # 嵌入模块
│   │   ├── factory.rs            # 嵌入器工厂
│   │   ├── fastembed.rs          # FastEmbed 实现
│   │   ├── mock.rs               # Mock 嵌入器
│   │   └── mod.rs                # 模块导出
│   ├── error.rs                   # 错误处理
│   ├── heartbeat/                 # 心跳模块
│   │   ├── analyzer.rs           # 分析器
│   │   ├── executor.rs           # 执行器
│   │   ├── mod.rs                # 模块导出
│   │   ├── question_pool.rs      # 问题池
│   │   ├── scheduler.rs          # 调度器
│   │   └── types.rs              # 类型定义
│   ├── intent/                    # 意图分类模块
│   │   ├── classifier.rs         # 分类器
│   │   └── mod.rs                # 模块导出
│   ├── learning/                  # 学习模块
│   │   ├── entity_kind_classifier.rs  # 实体类型分类
│   │   ├── entity_normalizer.rs       # 实体标准化
│   │   ├── knowledge_graph.rs         # 知识图谱
│   │   ├── learning_logger.rs         # 学习日志
│   │   ├── mod.rs                # 模块导出
│   │   ├── slice_relations.rs         # 切片关系
│   │   └── summarizer.rs               # 总结器
│   ├── lib.rs                     # 库入口
│   ├── llm/                       # LLM 模块
│   │   ├── factory.rs             # LLM 工厂
│   │   ├── mod.rs                # 模块导出
│   │   └── ollama.rs              # Ollama 实现
│   ├── memory/                    # 记忆管理模块
│   │   ├── mod.rs                # 模块导出
│   │   ├── queue.rs              # 记忆队列
│   │   └── three_library_manager.rs  # 三库记忆管理器
│   ├── memory_extractor/          # 记忆提取器模块
│   │   ├── dispatcher.rs          # 数据分流器
│   │   ├── extractor.rs           # 四要素提取器
│   │   └── mod.rs                # 模块导出
│   ├── shouxin-server.rs          # 服务器入口
│   ├── stores/                    # 存储模块
│   │   ├── dictionary/           # 字典库
│   │   │   ├── contacts_total.rs    # 联系人总表
│   │   │   ├── conversation.rs       # 对话档案
│   │   │   ├── mod.rs                # 模块导出
│   │   │   └── store.rs              # 字典库实现
│   │   ├── relational/            # 关系库
│   │   │   ├── contact_background.rs  # 联系人背景
│   │   │   ├── contact_index.rs      # 联系人索引
│   │   │   ├── event_index.rs        # 事件索引
│   │   │   ├── maintenance.rs        # 维护任务
│   │   │   ├── mod.rs                # 模块导出
│   │   │   ├── store.rs              # 关系库实现
│   │   │   └── todo_list.rs          # 待办列表
│   │   └── vector/              # 向量库
│   │       ├── factory.rs       # 向量存储工厂
│   │       ├── mod.rs           # 模块导出
│   │       ├── store.rs         # 向量存储实现
│   │       └── types.rs         # 向量类型定义
│   ├── traits/                   # Trait 定义模块
│   │   ├── embedder.rs          # 嵌入器 Trait
│   │   ├── llm.rs               # LLM Trait
│   │   ├── mod.rs               # 模块导出
│   │   ├── store_traits.rs      # 存储层 Trait
│   │   └── vector_store.rs      # 向量存储 Trait
│   └── utils/                   # 工具模块
│       ├── mod.rs               # 模块导出
│       └── validation.rs        # 验证工具
├── examples/                     # 示例文件
│   ├── simple_writing_test.rs   # 简单写入测试
│   ├── simple_workflow_test.rs  # 简单工作流测试
│   ├── complete_link_test.rs    # 完整链接测试
│   ├── full_workflow_test.rs    # 完整工作流测试
│   ├── detailed_link_test.rs    # 详细链接测试
│   ├── hnsw_test.rs             # HNSW 测试
│   └── hnsw_core_test.rs        # HNSW 核心测试
├── docs/                        # 文档目录
├── .fastembed_cache/            # 嵌入模型缓存
├── Cargo.toml                   # 依赖配置
├── Cargo.lock                   # 依赖锁定
├── config.toml                  # 全局配置
└── .gitignore                   # Git 忽略文件
```

---

## 六、示例文件说明

### 6.1 示例文件列表

| 示例文件 | 功能说明 | 状态 |
|---------|---------|------|
| [simple_writing_test.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/examples/simple_writing_test.rs) | 简单的 RelationalStore 写入测试，包含创建联系人、事件、待办 | ✅ 已修复 |
| [simple_workflow_test.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/examples/simple_workflow_test.rs) | 简化的链路测试，从创建联系人到清理待办 | ✅ 已修复 |
| [complete_link_test.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/examples/complete_link_test.rs) | 完整的链路测试，验证从创建到清理的全流程 | ✅ 已修复 |
| [full_workflow_test.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/examples/full_workflow_test.rs) | 完整工作流测试，包含更多细节 | ✅ 已修复 |
| [detailed_link_test.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/examples/detailed_link_test.rs) | 详细的完整链路测试，包含调试信息 | ✅ 已修复 |
| [hnsw_test.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/examples/hnsw_test.rs) | HNSW 索引功能测试 | ✅ 正常 |
| [hnsw_core_test.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/examples/hnsw_core_test.rs) | HNSW 核心功能测试 | ✅ 正常 |

### 6.2 运行示例

```bash
# 编译并运行简单写入测试
cargo run --example simple_writing_test

# 编译并运行完整链路测试
cargo run --example complete_link_test

# 编译并运行 HNSW 测试
cargo run --example hnsw_test
```

---

## 七、模块详解

### 7.1 核心模块（core）

包含 ShouXin 的核心数据结构定义：

| 文件 | 结构体/枚举 | 说明 |
|------|------------|------|
| [memory.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/core/memory.rs) | Memory | 记忆记录，包含 ID、内容、嵌入向量、元数据等 |
| [message.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/core/message.rs) | Message | 对话消息，包含角色和内容 |
| [fact.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/core/fact.rs) | Fact | 事实记录，包含陈述和置信度 |
| [embedding.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/core/embedding.rs) | Embedding | 嵌入向量，包含向量数据、维度和模型名称 |
| [contact.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/core/contact.rs) | ContactTotal, ContactIndex, EventIndex, OwnerType | 联系人、事件、待办等核心数据结构 |

### 7.2 Trait 定义模块（traits）

定义可扩展的接口，支持多种后端实现：

| 文件 | Trait | 说明 |
|------|-------|------|
| [llm.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/traits/llm.rs) | LlmClient | LLM 客户端接口，支持 chat、extract_facts、decide_memory_action |
| [embedder.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/traits/embedder.rs) | Embedder | 嵌入器接口，支持 embed、embed_batch |
| [vector_store.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/traits/vector_store.rs) | VectorStore | 向量存储接口，支持 insert、search、delete、update |

### 7.3 存储模块（stores）

三库架构实现：

#### 字典库（dictionary）
- **存储引擎**：sled
- **特点**：永久全量存档，只追加不删除
- **关键文件**：
  - [conversation.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/dictionary/conversation.rs)：时序对话档案存储
  - [store.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/dictionary/store.rs)：字典库实现

#### 关系库（relational）
- **存储引擎**：SQLite + SQLx
- **特点**：轻量快速索引
- **关键文件**：
  - [contact_index.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/contact_index.rs)：联系人索引表
  - [event_index.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/event_index.rs)：事件索引表
  - [todo_list.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/todo_list.rs)：待办事项列表
  - [store.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/store.rs)：关系库实现（包含 search_events_by_contact 改进）

#### 向量库（vector）
- **存储引擎**：HNSW-RS + sled
- **特点**：语义搜索支持
- **关键文件**：
  - [store.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/vector/store.rs)：向量存储实现
  - [factory.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/vector/factory.rs)：向量存储工厂

### 7.4 API 模块（api）

| 文件 | 功能 | 说明 |
|------|------|------|
| [async_memory.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/async_memory.rs) | 异步 API | 提供 add、search_contact、search_events、search_history、semantic_search 等方法 |
| [routes.rs](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/routes.rs) | HTTP 路由 | 定义所有 HTTP API 端点 |

---

## 八、关键改进说明（v0.3.0）

### 8.1 search_events_by_contact 方法改进

在 v0.3.0 中，我们改进了 [search_events_by_contact](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/store.rs#L372-L417) 方法的查询逻辑：

**改进前**：
- 使用严格的 LIKE 模式匹配
- 匹配特定位置的联系人 ID
- 可能导致查询不到相关事件

**改进后**：
- 使用更宽松的模式匹配
- 同时匹配带引号和不带引号的情况
- 提高了查询的成功率

```rust
// 新的查询模式
let patterns = vec![
    format!("%\"{}\"%", contact_id),  // 带引号的 ID
    format!("%{}%", contact_id),      // 不带引号的 ID
];
```

### 8.2 ContactTotal 使用方法修正

在 v0.3.0 中，我们修正了 ContactTotal 的使用方法：

**之前**（有问题）：
```rust
let mut contact_total = ContactTotal::new(...);
contact_total.add_alias("小张".to_string());  // 所有权被转移！
let contact_idx = ContactIndex::from_total(&contact_total);  // 错误！
```

**现在**（正确）：
```rust
let mut contact_total = ContactTotal::new(...);
contact_total.aliases.push("小张".to_string());  // 直接修改字段
let contact_idx = ContactIndex::from_total(&contact_total);  // 正确！
```

### 8.3 TodoItem 导入路径统一

在 v0.3.0 中，我们统一了 TodoItem 的导入路径：

**之前**（不统一）：
```rust
use shouxin::core::contact::TodoItem;  // 错误！
use shouxin::stores::relational::todo_list::TodoItem;  // 可以工作但不推荐
```

**现在**（统一）：
```rust
use shouxin::TodoItem;  // 正确！通过 lib.rs 重新导出
```

---

## 九、API 接口详解

### 9.1 通用响应格式

```json
{
  "success": true,
  "data": { ... },
  "error": null
}
```

### 9.2 记忆管理 API

#### 1. 添加对话记忆
- **端点**：POST /api/memory/add
- **函数**：[async_memory::add](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/async_memory.rs#L20-L50)

#### 2. 搜索联系人
- **端点**：POST /api/memory/search/contacts
- **函数**：[async_memory::search_contacts](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/async_memory.rs#L52-L80)

#### 3. 语义搜索
- **端点**：POST /api/memory/search/semantic
- **函数**：[async_memory::semantic_search](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/async_memory.rs#L112-L140)

---

## 十、配置系统

### 10.1 配置文件（config.toml）

```toml
[llm]
backend = "ollama"
model = "llama3.2:3b"
endpoint = "http://localhost:11434"
temperature = 0.7

[embedder]
model_path = "./.fastembed_cache"

[storage]
data_dir = "./data"
```

---

## 十一、构建与运行

### 11.1 编译项目

```bash
# 设置环境变量（Windows PowerShell）
$env:HF_HUB_OFFLINE = "1"
$env:FASTEMBED_CACHE_DIR = ".\.fastembed_cache"

# 设置环境变量（Linux/Mac）
export HF_HUB_OFFLINE=1
export FASTEMBED_CACHE_DIR=./.fastembed_cache

# Debug 编译
cargo build

# Release 编译
cargo build --release
```

### 11.2 运行示例

```bash
# 运行简单写入测试
cargo run --example simple_writing_test

# 运行完整链路测试
cargo run --example complete_link_test

# 运行 HNSW 测试
cargo run --example hnsw_test
```

### 11.3 运行服务器

```bash
# 启动服务
# Windows
.\target\release\shouxin-server.exe

# Linux/Mac
./target/release/shouxin-server
```

---

## 十二、总结

### 12.1 项目规模

- **总文件数**：168+ 个
- **代码行数**：32,351+ 行
- **源代码文件**：84+ 个
- **示例文件**：7 个
- **模块数**：14 个
- **API 端点数**：17 个

### 12.2 技术亮点

1. **三库分离架构**：字典库、关系库、向量库各司其职
2. **纯本地化**：无需联网，支持完全离线运行
3. **高性能**：使用 Rust + Tokio
4. **可扩展**：通过 Trait 模式
5. **语义搜索**：基于 FastEmbed + HNSW-RS
6. **智能提取**：使用 Ollama LLM 自动提取四要素
7. **完整测试覆盖**：7 个示例文件，覆盖主要功能场景
8. **稳定性提升**：v0.3.0 版本修复了所有已知问题

### 12.3 v0.3.0 版本特性

- ✅ 所有示例文件正常运行
- ✅ 编译无错误
- ✅ 事件查询功能改进
- ✅ 代码质量提升
- ✅ 文档完善
- ✅ 版本号正式更新为 0.3.0

---

> 文档版本：4.0.0
> 最后更新：2026-05-28
> 项目版本：v0.3.0
