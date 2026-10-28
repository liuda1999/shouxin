# 守心 ShouXin

**本地优先、隐私至上的智能体记忆子系统。**

守心不是给人用的笔记软件，而是给智能体额外挂载的一套记忆系统：智能体照常对话，顺手把对话内容丢给守心，
守心在后台把它拆解成**人物、事件、待办、知识**，写入三套存储，并按需做语义检索、主动提醒、联系人资料维护。

全程本地运行：LLM 推理用内置的 `llama.cpp`，文本嵌入用本地 ONNX 模型，**不需要联网、不需要云服务、不需要 Ollama**。

```
你的智能体 ──对话──▶ POST /api/memory/add ──▶ 落盘 + 入队（立即返回，不阻塞）
                                                    │
                                                    ▼（后台异步）
                              llama.cpp 抽取 ──▶ 人物 / 事件 / 待办 / 知识
                                                    │
                              ┌─────────────┬───────┴───────┐
                              ▼             ▼               ▼
                         关系库 SQLite   向量库 HNSW     字典库 Sled
                        （结构化索引）  （语义检索）   （原文，永不删除）
                                                    │
你的智能体 ◀──检索/提醒── GET /api/memory/search/* + /api/heartbeat/*
```

---

## 核心特性

| 能力 | 说明 |
|------|------|
| **异步抽取** | 写入只做落盘 + 入队，毫秒级返回；抽取在后台队列中进行，失败可重放 |
| **三库存储** | 关系库（结构化）+ 向量库（语义）+ 字典库（原文，永不删除），写入走强事务 + 补偿 |
| **本地推理** | 内置 `llama.cpp` 推理 GGUF，纯 CPU，无需 GPU / 无需 Ollama |
| **本地嵌入** | FastEmbed + ONNX（`multilingual-e5-base`），中文语义检索；模型缺失时自动降级为离线哈希嵌入 |
| **语义检索** | 事件 / 联系人 / 学习素材 / 历史对话多路检索 + 智能融合 |
| **待办管理** | 自动从对话中识别待办与截止时间，支持完成、更新、冷热归档 |
| **主动心跳** | 自适应频率的后台巡检：待办提醒、联系人资料维护、待确认问题 |
| **时间解析** | 中文相对时间（"下周三""月底"）本地解析，不依赖模型给日期 |
| **热冷分层** | 事件/联系人/待办按活跃度归档，冷数据仍可检索但降一级优先级 |
| **文件入库** | 上传文本/文档 → 提取文本 → 切片索引 → 可检索 |
| **多用户隔离** | 每个 owner（用户/智能体）独立命名空间，越权访问不泄露存在性 |

---

## 快速开始

### 0. 前置依赖

- **Rust** ≥ 1.75
- **C++ 工具链**：Windows 需 VS 2022 Build Tools；Linux 需 `build-essential`
- **CMake** ≥ 3.20 + **Ninja**

### 1. 准备模型（约 5.7 GB，必须手动下载）

模型体积大，不进 Git 仓库。**推理模型**和**嵌入模型**要分别下载：

```bash
mkdir -p models

# 推理模型 4.6 GB（国内建议用内置分段下载脚本，直连单线程常被限速到几十 KB/s）
python scripts/download_gguf.py \
  "https://hf-mirror.com/unsloth/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf" \
  "models/gemma-4-E4B-it-Q4_K_M.gguf" 32

# 嵌入模型 1.1 GB
bash scripts/fetch_embedding_model.sh
```

详细说明（含校验、目录布局、离线降级）见 **[docs/MODEL_SETUP.md](docs/MODEL_SETUP.md)**。

### 2. 构建

```bash
# Windows（推荐：一键脚本，已封装全部环境坑）
bash scripts/build_gguf.sh

# Linux / macOS
cargo build --release --features gguf
```

### 3. 运行

```bash
./target/debug/shouxin-server          # 默认 127.0.0.1:3000
# 或
./target/release/shouxin-server --host 0.0.0.0:3000
```

启动成功的关键日志：

```
[GGUF] 初始化: GPU=false, n_gpu_layers=0, context=4096, threads=16
GGUF model loaded successfully
Embedder initialized (backend: fastembed, model: intfloat/multilingual-e5-base)
Server listening on 127.0.0.1:3000
```

### 4. 验证

```bash
# 健康检查
curl http://127.0.0.1:3000/health

# 丢一段对话进去
curl -X POST http://127.0.0.1:3000/api/memory/add \
  -H "Content-Type: application/json" \
  -d '{"conversation":"下周三和张总开会讨论预算，下周五前给出方案。","user_id":"default_user"}'

# 查抽取状态（processing → completed，约 1 分钟）
curl http://127.0.0.1:3000/api/memory/extract_status/<conversation_id>

# 查联系人 / 待办 / 语义检索
curl -X POST http://127.0.0.1:3000/api/memory/search/contacts \
  -H "Content-Type: application/json" -d '{"query":"张","limit":5,"user_id":"default_user"}'
curl http://127.0.0.1:3000/api/todos
```

---

## 配置

配置文件默认 `./config.toml`，也可用环境变量指定：

```bash
SHOUXIN_CONFIG=./config.prod.toml ./shouxin-server
```

关键配置项：

```toml
[llm]
primary    = "gguf"      # gguf | ollama | api
max_tokens = 1024
timeout_secs = 1800      # 抽取链路整轮超时（纯 CPU 时别开太小）

[llm.gguf]
model_path   = "./models/gemma-4-E4B-it-Q4_K_M.gguf"
context_size = 4096
threads      = 16        # 建议 = 物理核心数
batch_size   = 2048      # prompt 分块预填充的块大小
use_gpu      = false     # false = 纯 CPU（内存驻留，不卸载到 GPU）
n_gpu_layers = 0

[embedder]
model_path       = ".fastembed_cache"
model            = "intfloat/multilingual-e5-base"
backend          = "fastembed"   # fastembed | hash
fallback_to_hash = true          # 模型缺失时降级，不阻塞启动
```

> **`threads` 建议设为物理核心数**（超线程对 llama.cpp 通常无益甚至更慢）。
> 更多配置项见 `config.toml` 内注释与 [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)。

---

## 性能基线（纯 CPU）

实测环境：AMD Ryzen 9 9950X（16 核 / 32 线程）、61.6 GB DDR5、无 GPU 参与。

| 指标 | 数值 | 备注 |
|------|------|------|
| 生成速度 · Gemma-4-E4B Q4_K_M（默认模型） | **≈ 16 tok/s** | 模型 4.6 GB |
| 生成速度 · Llama-3.2-3B Q5_K_M | ≈ 24 tok/s | 模型 2.3 GB，越小越快 |
| 生成速度 · ctx 8192 | 同上 | 短提示下与 4K 无差异；上下文填满时注意力开销会上升 |
| 单次抽取端到端（3 人 + 多待办 + 多事件） | **≈ 33 s**（Gemma）/ 70 s（3B） | 含 1 次主抽取 + 多轮人名校验 |
| 单次抽取（走正则降级） | ≈ 10 s | 无 LLM 调用 |
| 嵌入（e5-base, 768 维） | 毫秒级 | ONNX CPU |

> 数字会随 CPU、模型大小、量化方式变化。基准工具：`cargo run --release --features gguf --bin bench_gguf -- --threads 16`
> 方法论与踩坑记录见 **[docs/PERFORMANCE.md](docs/PERFORMANCE.md)**。

---

## API 一览

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/health` | 健康检查 |
| POST | `/api/memory/add` | 写入一段对话（立即返回 conversation_id） |
| GET | `/api/memory/extract_status/:id` | 查询抽取进度与结果 |
| POST | `/api/memory/replay/:id` | 重放单条对话的抽取 |
| POST | `/api/memory/replay_failed` | 批量重放失败的对话 |
| POST | `/api/memory/search/semantic` | 语义检索（向量） |
| POST | `/api/memory/search/events` | 事件检索 |
| POST | `/api/memory/search/contacts` | 联系人检索 |
| POST | `/api/memory/search/history` | 历史对话检索 |
| POST | `/api/search/smart` | 多路融合检索 |
| GET/POST | `/api/todos` | 待办列表 / 新建 |
| PUT | `/api/todos/:id/complete` | 完成待办 |
| DELETE | `/api/todos/:id` | 删除待办 |
| GET | `/api/events/pending_merges` | 待确认的事件合并 |
| POST | `/api/events/pending_merges/:id/resolve` | 确认合并（merge / create / dismiss） |
| POST | `/api/files/upload` | 上传文件并入索引 |
| GET | `/api/files/:id` · `/api/files/:id/text` | 文件元数据 / 提取的文本 |
| GET | `/api/heartbeat/status` · `/api/heartbeat/pending_questions` | 心跳状态 / 待确认问题 |
| POST | `/api/heartbeat/trigger` · `/api/heartbeat/answer` | 手动触发心跳 / 回答问题 |
| GET | `/api/virtual_entities` | 虚拟联系人（人设）列表 |

完整请求/响应示例见 **[docs/API.md](docs/API.md)**。

---

## 测试

```bash
cargo test --lib      # 69 项单元测试（快，不加载模型）
cargo test --tests    # 91 项集成测试（会加载真实 ONNX 嵌入模型）
```

当前基线：**160 passed / 0 failed**。

---

## 项目结构

```
ShouXin/
├── src/
│   ├── api/              # HTTP 路由与处理器
│   ├── config/           # 配置加载与结构定义
│   ├── core/             # 领域模型（事件/联系人/待办/记忆/事实）
│   ├── embedder/         # 嵌入器（fastembed ONNX / 离线哈希）
│   ├── llm/              # LLM 客户端（gguf / ollama / api + 工厂 + 管理）
│   ├── memory_extractor/ # 抽取器（LLM 提示词 + 解析 + 正则降级）
│   ├── service/          # 业务编排（后台队列、心跳、事务写入）
│   ├── stores/           # 三库实现（relational / vector / dictionary）
│   ├── traits/           # 抽象接口
│   ├── utils/            # 时间解析、分词、工具函数
│   ├── bin/bench_gguf.rs # 推理基准工具
│   └── shouxin-server.rs # 服务入口
├── tests/                # 集成测试
├── scripts/              # 构建 / 模型下载 / 编码修复 / 基准脚本
├── docs/                 # 文档
├── migrations/           # SQLite 迁移
└── config.toml           # 默认配置
```

---

## 已知限制

1. **多模态未启用**：`mmproj-BF16.gguf` 视觉塔已预留但不加载，抽取链路是纯文本。
2. **事件合并需人工确认**：相似度落在 0.75–0.90 的事件不会自动合并，进 `pending_merges` 等确认。
3. **整轮抽取耗时**：小模型（3B）在 CPU 上做多轮校验需要数十秒，属预期；追求速度可换更小模型或减少人名逐条校验。
4. **首次构建较慢**：要编译整个 llama.cpp（Windows 约 5–10 分钟，之后增量编译 1 分钟内）。

---

## 文档

| 文档 | 内容 |
|------|------|
| [docs/MODEL_SETUP.md](docs/MODEL_SETUP.md) | 推理模型 / 嵌入模型下载与目录布局 |
| [docs/BUILD.md](docs/BUILD.md) | 构建指南与常见编译错误 |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | CPU 推理性能：基准方法、根因分析、调优 |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | 排障手册 |
| [docs/API.md](docs/API.md) | 接口详解 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构与三库设计 |

---

## 许可

本仓库尚未声明开源许可证，使用与分发前请联系作者。

模型文件遵循各自上游许可：`gemma-4-E4B-it` 遵循 Google Gemma Terms of Use，
`multilingual-e5-base` 为 MIT。
