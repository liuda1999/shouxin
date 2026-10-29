<p align="center">
  <img src="assets/logo.png" alt="守心 ShouXin" width="200">
</p>

# 守心 ShouXin

**本地优先、隐私至上的智能体记忆子系统。**

> **适用人群：深度本地智能体用户。** 如果你只是想让 AI 记几句话，这个项目**不是**给你准备的。
> 它面向的是一类人：长期在自己的机器上跑智能体、希望记忆完全落在本地、愿意为数据主权投入硬件与折腾成本。
>
> **这是一个重代码项目。** 仓库里没有安装包、没有一键部署脚本、没有图形界面；
> 要跑起来，你得自己准备 Rust + C++ 工具链、手动下载数 GB 的模型、编译 llama.cpp、调配置。
> 它更像"一套可编程的记忆基础设施"，而不是一个开箱即用的产品。
>
> **用之前请先算物理资源账。** 下面按运行方式给了资源档位，请对着自己机器的实际情况选档，
> **不要用别人的跑分当标准** —— 推理速度取决于你的 CPU 架构、内存带宽、模型体积与量化方式，
> 差异可能有数倍。

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

### 1. 准备模型（必须手动下载，不进 Git）

模型体积大，不进 Git 仓库。**推理模型**和**嵌入模型**要分别下载。推理模型提供两档：

| 档位 | 模型 | 体积 | 特点 |
|------|------|------|------|
| **默认（标准）** | `gemma-4-E4B-it-Q4_K_M` | 4.6 GB | 抽取质量更好，推荐 |
| **低配 3B** | `Ministral-3-3B-Reasoning-2512-Q8_0` | 3.4 GB | 更省内存、更快起步，用 `config.3b.toml` 启用 |

```bash
mkdir -p models

# ① 默认推理模型 4.6 GB（国内建议用内置分段下载脚本，直连单线程常被限速到几十 KB/s）
python scripts/download_gguf.py \
  "https://hf-mirror.com/unsloth/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf" \
  "models/gemma-4-E4B-it-Q4_K_M.gguf" 32

# ② 嵌入模型 1.1 GB（必需）
bash scripts/fetch_embedding_model.sh
```

**低配 3B 方案**（可选，二选一）：

```bash
python scripts/download_gguf.py \
  "https://hf-mirror.com/unsloth/Ministral-3-3B-Reasoning-2512-GGUF/resolve/main/Ministral-3-3B-Reasoning-2512-Q8_0.gguf" \
  "models/Ministral-3-3B-Reasoning-2512-Q8_0.gguf" 32

# 用低配配置启动（模型路径已指向 Ministral）
SHOUXIN_CONFIG=./config.3b.toml ./target/release/shouxin-server
```

> 视觉塔 `mmproj` 目前不加载（抽取为纯文本），仅在需要多模态时预留。

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

配置文件默认 `./config.toml`。**支持三种推理后端**（内置 llama.cpp / Ollama / OpenAI 兼容 API），
**上下文长度可在配置里自由调节**，完整说明见 **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)**。

覆盖顺序（后者优先）：

1. `config.toml`
2. 同目录的 `config.local.toml`（本地私有覆盖，已在 `.gitignore` 中）
3. 环境变量，前缀 `SHOUXIN__`，层级用双下划线：`SHOUXIN__SERVER__PORT=3100`
4. 命令行参数，如 `--host 0.0.0.0:8080`

```bash
SHOUXIN_CONFIG=./config.3b.toml ./shouxin-server   # 切换到低配 3B 档
SHOUXIN__SERVER__PORT=3100 ./shouxin-server        # 临时改端口
```

**改完配置建议跑一次一致性检查**（会发现"写了但不生效"的死配置）：

```bash
python scripts/check_config.py config.toml config.3b.toml
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
use_gpu      = false     # 默认纯 CPU（内存驻留，不卸载到 GPU）
n_gpu_layers = 0
gpu_max_cpu_percent = 60 # 仅 GPU 模式生效：CPU 占用上限（核心数百分比）

[embedder]
model_path       = ".fastembed_cache"
model            = "intfloat/multilingual-e5-base"
backend          = "fastembed"   # fastembed | hash
fallback_to_hash = true          # 模型缺失时降级，不阻塞启动

[server]
host = "127.0.0.1"  # 默认只监听本机；--host 优先级更高
port = 3000
```

> **`threads` 建议设为物理核心数**（超线程对 llama.cpp 通常无益甚至更慢）。
> 更多配置项见 `config.toml` 内注释与 [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)。

### 资源占用原则（重要）

守心是**后台服务**，必须与其它进程共存，因此对资源有两条硬约束：

1. **默认纯 CPU，不占显存**
   `use_gpu = false` 是默认值（配置缺省、代码缺省都是 false）。
   模型全部驻留内存，不向 GPU 卸载任何层；`n_gpu_layers` 强制为 0。
   需要 GPU 时必须显式改 `use_gpu = true` 并用 `--features gguf-cuda` 编译。

2. **GPU 模式下 CPU 占用不超过 60%**
   即使推理主要跑在 GPU 上，llama.cpp 仍会开线程做数据搬运与采样。
   实际线程数 = `min(threads, **物理核数** × gpu_max_cpu_percent%)`（向下取整，默认 60%）。
   16 物理核机器上最多用 9 个线程，绝不会吃满；
   生效时日志会打印：`[GGUF] GPU 模式限制 CPU 占用：threads 16 -> 9（核心 16，上限 60%）`。
   （用物理核而非逻辑核：32 逻辑线程 × 60% = 19 会超过 16 个物理核，等于还是超载。）

纯 CPU 模式下线程数按 `threads` 配置，并且不会超过核心数。

---

## 资源需求与档位建议

守心的资源开销主要由**推理方式**决定。三种方式按"从省到贵"排列：

| 运行方式 | 需要什么 | 适合谁 |
|----------|----------|--------|
| **云端 API**（OpenAI 兼容） | 只要 1–2 GB 内存，无需模型文件 | 机器弱、或想省电省内存；介意联网则不适合 |
| **Ollama 本地服务** | Ollama + 一个 3B~7B 模型（内存 4–8 GB） | 已有 Ollama、想复用它管理模型 |
| **内置 llama.cpp（默认）** | 纯 CPU 推理，模型 3.4–4.6 GB | 完全离线、数据不出本机 |

内置推理（纯 CPU）按模型档位估算：

| 档位 | 模型体积 | 峰值内存 | 磁盘占用 | CPU 建议 | 体验预期 |
|------|----------|----------|----------|----------|----------|
| **低配 3B** | ≈3.4 GB（Q8_0） | ≈5–7 GB | ≈6–8 GB | 4 核起 | 可用，单次抽取几十秒到一两分钟 |
| **标准 4B** | ≈4.6 GB（Q4_K_M） | ≈7–10 GB | ≈7–10 GB | 8 核起 | 推荐，质量更稳 |
| **7B 及以上** | ≥4.5 GB（Q4_K_M） | ≥10 GB | ≥10 GB | 12 核起 | 纯 CPU 明显吃力，建议配 Ollama + GPU 或走云端 API |

> 内存 = 模型 + KV 缓存 + 索引（向量库/嵌入模型约占 1.5 GB）；
> 磁盘 = 模型 + 嵌入模型（1.1 GB）+ 数据增长。
>
> **上下文越长，内存与耗时越高**：`context_size` 从 4096 调到 8192 时 KV 缓存约翻倍，
> 长对话场景才有必要加大（见 [docs/CONFIGURATION.md](docs/CONFIGURATION.md)）。

### 想知道自己机器能跑多快？

不要参考别人的数字，直接测：

```bash
bash scripts/bench_cpu.sh                 # 线程扫描，输出 tok/s
# 或指定模型
bash scripts/bench_cpu.sh models/Ministral-3-3B-Reasoning-2512-Q8_0.gguf
```

注意：**不要在服务运行中跑基准**，两者抢 CPU 会让数字严重失真。

> 我们在开发机上做过的一组参考数字只用于回归对比，**不作为选型标准**，
> 详见 [docs/PERFORMANCE.md](docs/PERFORMANCE.md)。
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
| POST | `/api/memory/search/contacts` | 联系人检索（含人设 / 别名 / 关系 / `is_virtual`） |
| GET | `/api/contacts/:id/detail` | **联系人全息详情**：人设 + 关联联系人 + 关联事件 + 相关记忆原文 |
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
cargo test --lib      # 单元测试（快，不加载模型）
cargo test --tests    # 集成测试（会加载真实 ONNX 嵌入模型）

# 全量 API 冒烟（覆盖 routes.rs 注册的全部端点，需先启动服务）
bash scripts/api_smoke_test.sh
BASE=http://127.0.0.1:3001 USER_ID=smoke bash scripts/api_smoke_test.sh   # 指定地址/隔离用户

# 200 条验收压测（自造语料：长/短/公式/人物关系/边界）
python scripts/stress_200.py submit --base http://127.0.0.1:3000 --user stress200
python scripts/stress_200.py poll   --base http://127.0.0.1:3000 --user stress200
python scripts/stress_200.py verify --base http://127.0.0.1:3000 --user stress200

# 批量生成联系人画像（心跳每轮只处理 3 个，需要多轮驱动）
python scripts/gen_personas.py --base http://127.0.0.1:3000 --user stress200 --max-rounds 40
```

压测结果与发现的问题见 **[docs/STRESS_TEST.md](docs/STRESS_TEST.md)**。

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
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | **配置详解**：gguf/Ollama/API 三种后端切换、上下文调节、阈值校准 |
| [docs/STRESS_TEST.md](docs/STRESS_TEST.md) | **200 条压测报告**：语料设计、检查项结论、暴露的缺陷 |
| [docs/MODEL_SETUP.md](docs/MODEL_SETUP.md) | 推理模型 / 嵌入模型下载与目录布局 |
| [docs/BUILD.md](docs/BUILD.md) | 构建指南与常见编译错误 |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | CPU 推理性能：基准方法、根因分析、调优 |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | 排障手册 |
| [docs/API.md](docs/API.md) | 接口详解 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构与三库设计 |

---

## 许可

本项目采用 **[PolyForm Noncommercial License 1.0.0](LICENSE)**：

- ✅ 允许：个人学习、研究、实验、业余项目，以及慈善 / 教育 / 公共研究 / 政府等非商业机构使用；
- ❌ **禁止任何商业用途**，包括但不限于商业产品集成、对外收费服务、企业内部商业运营；
- 分发或修改后再分发时，必须附带本许可证与 `Required Notice` 声明。

需要商业授权请另行联系作者。完整条款见 [LICENSE](LICENSE)。

模型与依赖的许可各自独立：`gemma-4-E4B-it` 遵循 Google Gemma Terms of Use，
`Ministral-3-3B` 遵循上游 Mistral 许可，`multilingual-e5-base` 为 MIT。
