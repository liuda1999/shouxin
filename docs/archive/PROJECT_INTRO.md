# 守心（ShouXin）v0.5.0 — 项目全貌

## 一、项目概述

**守心（ShouXin）** 是一个基于 Rust 构建的**本地优先、隐私至上**的智能记忆系统。它通过大语言模型从日常对话中自动提取结构化记忆——人物、事件、待办、学习素材——并以"三库架构"（字典库 + 关系库 + 向量库）持久化存储，支持语义搜索、事件生命周期管理、以及定期自主心跳维护。

核心设计理念：**用户的所有数据完全本地存储，不需联网，不需云服务，不需第三方。** 所有 AI 推理和嵌入计算均在本地 GPU 上完成。

---

## 二、技术栈

| 层次 | 技术选型 |
|------|----------|
| 语言 | **Rust 2021 Edition** — 零成本抽象、内存安全、原生异步 |
| 异步运行时 | **Tokio** — 全异步 I/O |
| Web 框架 | **Axum 0.7** — 高性能 HTTP 服务 |
| 关系数据库 | **SQLite 3** (sqlx) — WAL 模式，轻量高效 |
| KV 存储 | **Sled** — 嵌入式持久化键值存储（字典库 + 任务队列） |
| 向量检索 | **HNSW** (hnsw_rs) + **Embedded**（内置暴力搜索）— 双模式可选 |
| 文本嵌入 | **FastEmbed** (ort) — 本地 ONNX 推理，`multilingual-e5-base` |
| LLM 推理 | **llama-cpp-2** (GGUF) + **Ollama API** + **OpenAI 兼容 API** — 三后端可切换 |
| 安全 | **constant_time_eq** — 恒定时间令牌比较防时序攻击 |

---

## 三、系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                      HTTP API Layer                         │
│  POST /api/memory/add          POST /api/todos              │
│  POST /api/memory/search/*     GET/PUT/DELETE /api/todos/*  │
│  POST /api/files/upload        GET /api/files/list          │
│  POST /internal/...            GET /admin/llm/status        │
├─────────────────────────────────────────────────────────────┤
│                   Three-Library Memory Manager               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  字典库 (Sled) │  │ 关系库 (SQLite)│  │ 向量库 (HNSW) │      │
│  │  永久存储     │  │  轻量索引     │  │  语义搜索     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
├─────────────────────────────────────────────────────────────┤
│                    Memory Extractor                          │
│  LLM 解析 → 人物 / 事件 / 待办 / 事实 / 学习素材            │
│               ↓                                              │
│          DataDispatcher 分流到三库                            │
├─────────────────────────────────────────────────────────────┤
│                    Background Task Queue                     │
│  sled 持久化队列 → 异步事件提取 → 失败记录                    │
├─────────────────────────────────────────────────────────────┤
│                    Heartbeat System                          │
│  维护心跳 (SQL) + LLM 智能心跳 (分析/合并/去重)              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  定时调度器   │  │  分析器       │  │  问题池       │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
├─────────────────────────────────────────────────────────────┤
│                     LLM Integration                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  GGUF (本地)  │  │  Ollama API  │  │  OpenAI API  │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│        LlmClientManager (主/备自动切换 + 健康检查)           │
└─────────────────────────────────────────────────────────────┘
```

---

## 四、核心模块详解

### 4.1 三库记忆架构

这是系统的数据存储核心，三种数据库各司其职：

- **字典库（Sled）**：永久存储所有原始记忆数据（对话、联系人、事件、学习素材），类似"档案室"。
- **关系库（SQLite）**：存储轻量级索引（联系人索引、事件索引、待办列表、文件索引），支持快速检索和过滤，类似"目录卡片"。
- **向量库（HNSW 或 Embedded）**：存储文本的语义嵌入向量，支持余弦相似度搜索，类似"概念地图"。

**数据流转**：用户对话 → LLM 提取 → DataDispatcher 分流 → 三库同步写入。

### 4.2 事件提取与分类

系统从对话中自动提取**五类信息**：

| 类型 | 说明 | 示例 |
|------|------|------|
| 人物（Person） | 对话中提到的真实人名 | "王经理"、"李总" |
| 事件（Event） | 带时间信息的事项 | "明天下午3点开会" |
| 待办（Todo） | 需要完成的任务 | "记得续费域名" |
| 事实（Fact） | 陈述性知识 | "公司有500名员工" |
| 学习素材（Learning） | 有价值的知识片段 | "K8s 调度原理是..." |

**事件重要性分类**（三级）：

| 级别 | 数值 | 存储位置 | 说明 |
|------|------|----------|------|
| Critical | ≥ 0.95 | 热表 | 紧急/高影响事件 |
| Long-term | > 0.7 | 热表 + 冷归档 | 长期目标/计划 |
| Short-term | ≥ 0.5 | 热表 | 短期日常事件 |
| Low | < 0.5 | 可被清理 | 低价值信息 |

**热/冷分离机制**：
- 热表（hot）：存储近期活跃事件，限量 `long_term_hot_limit=200` / `short_term_hot_limit=100`
- 冷归档（archive）：超量事件自动按月归档，保留历史但降低查询开销

### 4.3 心跳系统

心跳是系统的**自主维护机制**，定期执行两类任务：

**维护心跳（纯 SQL，不调用 LLM）**：
- 清理过期事件和待确认合并
- 低重要性事件清理
- 联系人冷归档（180天无交互）
- 待办热/冷管理（>30个待办生成提醒，>50个归档陈旧待办）

**LLM 智能心跳（调用 LLM）**：
- 待决歧义分析（同名联系人、冲突信息）
- 联系人关系分析
- 知识冲突检测
- 实体标准化

**调度策略**：可配置固定间隔或自适应间隔（根据系统负载动态调整），支持内存阈值保护。

### 4.4 LLM 多后端支持

系统通过 `LlmClientManager` 统一管理 LLM 后端，支持**三种来源**：

| 后端 | 说明 | 适用场景 |
|------|------|----------|
| **GGUF** | 本地 llama.cpp 推理，CUDA 加速 | 完全离线部署 |
| **Ollama** | 本地 Ollama 服务 API | 便捷切换不同模型 |
| **API** | OpenAI 兼容 API | 使用云端模型 |

**亮点特性**：
- **主/备自动切换**：主后端故障时自动切换到备后端
- **健康检查**：定期检测后端可用性
- **心跳独立客户端**：主系统和心跳模块可使用不同的 LLM 后端和参数（`[llm.heartbeat]` 独立配置）
- **模型预加载**：启动时预热模型，减少首次请求延迟

### 4.5 后台任务队列

基于 Sled 持久化的异步任务队列，确保事件提取不阻塞 HTTP 响应：

- 对话提交后立即返回 `conversation_id`
- 提取任务入队，后台异步执行
- 失败时记录到 `extraction_error` 并标记 `failed`（当前不自动重试）
- 服务重启后从 sled 恢复未完成任务
- 通过 `/api/memory/extract_status/{id}` 查询提取进度

### 4.6 配置系统

支持**多层配置合并**（优先级从高到低）：

1. 环境变量（`SHOUXIN_*`）
2. `config.local.toml`（本地覆盖，不纳入版本控制）
3. `config.toml`（默认配置）

配置示例：

```toml
[llm]
primary = "gguf"              # 主 LLM 后端
secondary = "none"            # 备 LLM 后端
temperature = 0.7
max_tokens = 2048

[llm.gguf]
model_path = "./models/Llama-3.2-3B-Instruct-Q5_K_M.gguf"
context_size = 8192
n_gpu_layers = 99             # 全部层 GPU 加速

[llm.heartbeat]
backend = "gguf"              # 心跳独立 LLM 后端
temperature = 0.3
max_tokens = 500

[heartbeat]
enabled = true
base_interval_secs = 60       # 心跳间隔

[event]
long_term_hot_limit = 200     # 长期事件热表容量
short_term_hot_limit = 100    # 短期事件热表容量
```

---

## 五、API 端点总览

### 5.1 记忆管理

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/memory/add` | 添加对话，触发异步事件提取 |
| `GET` | `/api/memory/extract_status/{id}` | 查询提取进度 |
| `POST` | `/api/memory/search/contacts` | 搜索联系人（支持热/冷归档） |
| `POST` | `/api/memory/search/events` | 搜索事件（支持时间/重要性/状态过滤） |
| `POST` | `/api/memory/search/history` | 搜索历史对话 |
| `POST` | `/api/memory/search/semantic` | 语义搜索（向量检索） |

### 5.2 待办管理

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/todos` | 创建待办 |
| `GET` | `/api/todos` | 获取待办列表（支持状态/联系人过滤） |
| `GET` | `/api/todos/{id}` | 获取待办详情 |
| `PUT` | `/api/todos/{id}/complete` | 完成待办 |
| `DELETE` | `/api/todos/{id}` | 删除待办 |

### 5.3 文件管理

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/files/upload` | 上传文件（支持 txt/pdf/zip，最大 20MB） |
| `GET` | `/api/files/list` | 列出已上传文件 |
| `GET` | `/api/files/{id}` | 获取文件内容 |

### 5.4 管理端点

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/admin/llm/status` | LLM 后端状态（主/备/健康检查） |
| `POST` | `/admin/llm/switch` | 手动切换 LLM 后端 |
| `GET` | `/internal/health` | 内部健康检查 |

---

## 六、数据模型

### 6.1 对话（Conversation）
```
id, user_id, owner_type, owner_id, content, 
created_at, extraction_status, ...
```

### 6.2 事件（Event）
```
event_id, title, description, time, location, 
importance (0.0-1.0), importance_label (short_term/long_term/critical/low),
status, start_time, end_time, related_person_ids, ...
```

### 6.3 联系人（Contact）
```
contact_id, name, aliases, role, description, 
contact_type, profile, hot/cold status, ...
```

### 6.4 待办（Todo）
```
todo_id, title, description, deadline, importance, 
status, contact_ids, source_event_id, scope, ...
```

---

## 七、安全设计

| 措施 | 实现 |
|------|------|
| 防时序攻击 | `constant_time_eq` 令牌比较 |
| 本地优先 | 所有数据本地存储，不上传云端 |
| 参数化 SQL | sqlx 全参数化查询，防注入 |
| Token 验证 | Admin API 需要 Bearer Token |
| 配置隔离 | 敏感配置通过环境变量或 `config.local.toml`（gitignore） |

---

## 八、性能特征

- **LLM 推理**：GGUF 模式 CUDA 全 GPU 加速，~1s/请求（3B 模型）
- **向量检索**：HNSW 近似最近邻，查询延迟 < 10ms
- **异步提取**：事件提取不阻塞 HTTP 响应，后台队列处理
- **持久化恢复**：Sled 持久化队列，服务重启后自动恢复未完成提取
- **内存控制**：心跳调度器含内存阈值保护，低内存时自动跳过 LLM 心跳
- **热/冷分离**：事件和联系人自动归档，控制热表规模

---

## 九、部署方式

### 本地 GGUF 部署（推荐）

```bash
# 编译
cargo build --release --features gguf

# 配置
cp config.toml.example config.toml
# 编辑 config.toml：设置 primary = "gguf"

# 运行
cargo run --release --features gguf --bin shouxin-server
```

### 部署包结构

```
ShouXin_Complete_v0.5.0/
├── config.toml                  # 主配置文件
├── Cargo.toml / Cargo.lock     # Rust 依赖
├── src/                         # 源码 (~20个模块)
├── tests/                       # 测试脚本
├── models/                      # GGUF 模型文件
│   └── Llama-3.2-3B-Instruct-Q5_K_M.gguf (2.2 GB)
├── .fastembed_cache/            # 嵌入模型缓存
│   └── intfloat/multilingual-e5-base/ (4.4 GB)
├── docs/                        # 文档
├── examples/                    # 使用示例
└── migrations/                  # SQLite 迁移脚本
```

---

## 十、项目亮点总结

1. **完全本地运行**：GGUF 本地推理 + ONNX 本地嵌入，数据不出本机
2. **三库架构**：字典/关系/向量三层存储，兼顾持久性、检索速度和语义理解
3. **多 LLM 后端**：GGUF / Ollama / API 三种后端无缝切换，主/备自动故障转移
4. **事件热/冷分离**：长短期事件分类存储，自动归档，控制热表规模
5. **自主心跳维护**：定期清理、归档、去重、合并，系统自我维护
6. **异步事件提取**：HTTP 即时响应，LLM 提取后台执行，sled 持久化队列故障恢复
7. **多层配置**：config.toml → config.local.toml → 环境变量，灵活覆盖
8. **Rust 技术栈**：内存安全、零成本抽象、原生异步、编译为单一二进制
9. **安全优先**：防时序攻击、参数化查询、Token 验证、本地数据隔离
10. **模块化设计**：20+ 独立模块，清晰的 trait 接口，易于扩展和测试