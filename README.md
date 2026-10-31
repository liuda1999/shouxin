<p align="center">
  <img src="assets/logo.png" alt="守心 ShouXin" width="200">
</p>

# 守心 ShouXin

**给智能体挂载的本地记忆子系统：对话进得去，事实落得住，换种问法也能想起来。**

[![version](https://img.shields.io/badge/version-0.7.6-blue)](CHANGELOG.md)
[![license](https://img.shields.io/badge/license-PolyForm%20Noncommercial-orange)](LICENSE)
[![rust](https://img.shields.io/badge/rust-%E2%89%A51.75-informational)](docs/BUILD.md)
[![tests](https://img.shields.io/badge/tests-109%20unit%20%2B%2013%20suites-brightgreen)](docs/ARCHITECTURE.md)

守心不是笔记软件，也不是聊天前端。它是**给智能体额外挂载的一套记忆**：智能体照常对话，
把内容和时间戳丢给守心，守心在后台拆成**人物、事件、待办**，写进三套存储，
之后按语义或关键词取回，还会定期主动巡检（提醒待办、维护联系人）。

全程本地：推理用内置 `llama.cpp`，嵌入用本地 ONNX 模型，**不联网、不依赖云服务、不需要 Ollama**。

```
智能体 ──单条 / 批量 / 文件──▶ POST /api/memory/add · /batch · /files/upload
                                      │ 落盘 + 入队，毫秒返回
                                      ▼（后台异步，单并发）
                        llama.cpp 抽取 ──▶ 人物 / 事件 / 待办
                                      │
                 ┌────────────────────┼────────────────────┐
                 ▼                    ▼                    ▼
            关系库 SQLite         向量库 HNSW          字典库 Sled
          （结构化索引：       （事件 + 对话原文       （原文按会话日分页，
            联系人/事件/待办）     双路语义检索）         永不删除）
                 └────────────────────┼────────────────────┘
                                      ▼
   智能体 ◀── /api/search/smart ── 词法召回 + 语义兜底（换词也能找到）
          ◀── /api/heartbeat/* ── 主动巡检：待办提醒 / 画像维护 / 待确认问题
          ◀── /api/backup · /export ── 运维：一致性快照 / 全量导出
```

---

## 核心能力

**写入与抽取**

| 能力 | 说明 |
|------|------|
| 单条写入 | `POST /api/memory/add`，立即返回 `conversation_id`，可带历史 `timestamp` 导入旧数据 |
| 批量导入 | `POST /api/memory/batch`，单批 ≤500 条，逐条返回 id；实测 12 条接收耗时 1.7ms |
| 文件入库 | 上传文本/文档 → 提取 → 按 1200 字上限切块（句读处断开）→ 嵌入 → **融合检索直接可召回**（带文件名与相似度） |
| 异步抽取 | 后台队列单并发执行；`extract_status` 查进度，失败可 `replay` 重放 |
| 多轮对话 | `messages: [{role, content}]`，落库与抽取提示词都保留「我：/对方：」，用于判方向 |

**三库存储**

| 能力 | 说明 |
|------|------|
| 关系库 | SQLite 存联系人 / 事件 / 待办的结构化索引，支持按人、按时间、按关键词查 |
| 向量库 | HNSW 存事件描述**与对话原文**，换词提问也能召回（实测见下） |
| 字典库 | Sled 存对话原文，永不删除；按"会话日"分页，凌晨 4 点前归前一天 |
| 一致性 | 写入走强事务 + 补偿；向量写失败会回滚关系库，避免"索引说有、原文没有" |

**检索**

| 能力 | 说明 |
|------|------|
| 融合检索 | `POST /api/search/smart` 一次查联系人 / 事件 / 历史 / 待办，返回 `matched_terms` |
| 语义兜底 | 关键词命中不足时自动走向量召回，并回源字典库返回**原文**（带 `similarity`） |
| 实测召回 | 两套全新语料、问法不含原句关键词：修复前 **1/12**，修复后 **12/12** 与 **8/8** |
| 历史回溯 | `search/history` 支持 `query` + `limit`，按时间倒序取最新，不再只返回最旧 100 条 |

**联系人与待办**

| 能力 | 说明 |
|------|------|
| 联系人档案 | 姓名 / 别名 / 关系标签 / 画像 / 关联事件 / 原文证据，`/api/contacts/:id/detail` 一次取全 |
| 同名消歧 | 身份是 UUID，姓名不唯一；靠消歧标签 / 身份线索 / 别名多字段投票区分 |
| 虚拟角色 | 历史 / 神话 / 动漫 / 游戏角色独立建表（按作品唯一），与真人隔离；通过 `include_virtual=true` 共用联系人检索与详情 |
| 待办 | 自动识别待办与截止时间（中文相对时间本地解析），完成操作幂等，冷热归档 |
| 主动心跳 | 自适应频率巡检：待办提醒、画像维护、待确认问题；空转轮次毫秒级，不空烧 CPU |

**运行与运维**

| 能力 | 说明 |
|------|------|
| CPU 预算 | `[runtime].max_cpu_percent`（默认 60）：**整进程**占整机上限，Windows 作业对象硬限制 |
| 备份导出 | `POST /api/backup` 生成一致性快照（`VACUUM INTO`），`GET /api/export` 导出全量 JSON |
| 可观测 | 结构化日志、`/health`、抽取状态、心跳状态；模型输出杂质自动清洗 |
| 多用户隔离 | 每个 owner 独立命名空间，越权访问不泄露存在性 |

---

## 设计取舍

- **写入不阻塞，抽取慢是常态。** 4B 模型纯 CPU 单条约 30–60 秒。要快就换小模型、走 API 后端，
  或先批量灌入再等队列排空（约 2.8 条/分钟）。
- **原文是事实来源，索引只是投影。** 关系库和向量库都可以重建，字典库不删；出错时以原文为准。
- **关键词保精确，语义保"换词"。** 只做关键词会漏掉"办喜事/婚礼"这类同义表达，
  只做语义会把"工资"召回给"体检"；所以两路并用，语义只在关键词命中不足时兜底。
- **资源优先于速度。** 这是后台服务，必须和其它程序共存：CPU 有硬上限，空转不占资源。

---

## 技术栈

| 层 | 选型 | 版本 | 说明 |
|----|------|------|------|
| 语言 | Rust（edition 2021） | ≥ 1.75（开发机 1.99.1） | 全项目单一语言 |
| HTTP | `axum` + `tower-http` | 0.7 / 0.5 | 45 个注册端点 / 38 个唯一路径（另 /admin 2、/internal 2） |
| 异步运行时 | `tokio` | 1.x（full） | 工作线程栈 32 MB（llama.cpp 分词器需要） |
| 推理 | `llama-cpp-2`（llama.cpp 绑定） | `=0.1.146` | 纯 CPU，默认不编译 CUDA |
| 嵌入 | `fastembed` + ONNX Runtime | 4.9.1 | `intfloat/multilingual-e5-base`，缺模型降级哈希 |
| 关系库 | SQLite（`sqlx` + bundled `libsqlite3-sys`） | sqlx 0.8 | 无需系统装 SQLite |
| 向量库 | `hnsw_rs` | 0.3 | HNSW 近似最近邻 |
| 字典库 | `sled` | 0.34 | 对话原文，永不删除 |
| 配置 | `config` + `clap` | 0.14 / 4 | TOML + 环境变量 + 命令行 |
| 构建 | CMake ≥ 3.20 + Ninja | — | Windows 用 MSVC 2022 Build Tools；Linux/macOS 用 GCC/Clang |

> 依赖里没有 GPU 运行时、没有 Docker、没有外部数据库。**唯一的系统级依赖是 C++ 编译工具链**（用于编译 llama.cpp）。

> **适用人群**：长期在自己机器上跑智能体、要求记忆完全本地、愿意为数据主权投入硬件的用户。
> 仓库里没有安装包、没有图形界面；要跑起来得自备 Rust + C++ 工具链、手动下载数 GB 模型。
> 它更像"一套可编程的记忆基础设施"，而不是开箱即用的产品。

---

## 部署：从 GitHub 拉取到跑通

以下命令在 **Git Bash（Windows）/ bash（Linux、macOS）** 下执行，路径均相对仓库根目录。

### 0. 前置条件

| 项 | 要求 | 检查命令 |
|----|------|----------|
| Rust | ≥ 1.75（含 cargo） | `rustc --version` |
| C++ 工具链 | Windows：VS 2022 Build Tools（含 MSVC v143 + Windows SDK）；Linux：`build-essential`；macOS：Xcode CLT | `cl` / `g++ --version` |
| CMake | ≥ 3.20 | `cmake --version` |
| Ninja | 任意较新版本 | `ninja --version` |
| Python | ≥ 3.8 + `requests`（下载 / 压测脚本用） | `python --version` · `python -c "import requests"` |
| 磁盘 | ≥ 15 GB（模型 5.7 GB + 构建产物 + 数据） | — |
| 内存 | ≥ 8 GB（4B 档建议 10 GB） | — |

Windows 上如果不想手工配 MSVC 环境，直接用 `scripts/build_gguf.sh`（内部会调 `scripts/dev_env.sh` 自动探测，**不写死任何机器路径**）。

### 1. 克隆

```bash
git clone https://github.com/liuda1999/shouxin.git
cd shouxin
```

仓库本身约 5 MB（模型、数据、构建产物都在 `.gitignore` 里）。

### 2. 环境自检（强烈建议）

```bash
bash scripts/check_env.sh
```

它会逐项检查 Rust / CMake / Ninja / C++ 工具链 / 模型文件 / 端口占用，**缺什么直接给出修复命令**，不用等到编译报错才发现。

### 3. 下载模型（必须手动，不进 Git）

推理模型和嵌入模型要分别下载。推理模型两档选一：

| 档位 | 模型 | 体积 | 特点 |
|------|------|------|------|
| **默认（标准）** | `gemma-4-E4B-it-Q4_K_M` | 4.6 GB | 抽取质量更好，推荐 |
| **低配 3B** | `Ministral-3-3B-Reasoning-2512-Q8_0` | 3.4 GB | 更省内存、更快起步，用 `config.3b.toml` 启用 |

> **这两档只是预设，模型没有被写死。** 代码不认模型名、不校验哈希，任何 llama.cpp 支持的 GGUF 都能直接换
> （内置 llama.cpp 支持 126 个架构，含 Qwen / Phi / GLM / DeepSeek / Granite 等），量化格式任意（Q4/Q5/Q8）。
> 换的时候**两处要同步改**：`[llm.gguf].model_path`（对话抽取）和 `[llm.heartbeat].model`（画像与维护），
> 只改前者会让心跳继续加载旧模型。细节见 [docs/MODEL_SETUP.md §1.5](docs/MODEL_SETUP.md)。

```bash
# 下载脚本依赖 requests（一次性）
python -m pip install --upgrade requests

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

SHOUXIN_CONFIG=./config.3b.toml ./target/release/shouxin-server
```

> 视觉塔 `mmproj` 目前不加载（抽取为纯文本），仅在需要多模态时预留。
> 校验方式、目录布局、离线降级见 **[docs/MODEL_SETUP.md](docs/MODEL_SETUP.md)**。

### 4. 构建

```bash
# Windows（推荐：一键脚本，已封装 MSVC / CMake / Ninja / UTF-8 四个坑）
bash scripts/build_gguf.sh --release

# Linux / macOS
cargo build --release --features gguf
```

> `build_gguf.sh` **默认出 debug 构建**（产物在 `target/debug/`，首次约 2 分钟）。
> 生产部署请加 `--release`（产物在 `target/release/`，编译 llama.cpp 优化版，5–10 分钟）。
> 下面的运行命令按 release 写；如果你想先用 debug 快速验证，把路径换成 `target/debug/` 即可。

首次构建会编译整个 llama.cpp：Windows 约 5–10 分钟，之后增量编译 1 分钟内。

> **cmake / ninja 装在非标准位置怎么办**（便携版、`pip install cmake`、绿色版）：
> 用环境变量指给构建脚本，否则 `llama-cpp-sys` 的 build script 会报
> `failed to execute command: program not found`：
>
> ```bash
> export SHOUXIN_CMAKE="/path/to/cmake"
> export SHOUXIN_NINJA="/path/to/ninja"
> bash scripts/build_gguf.sh --release
> ```
>
> 其他可覆盖项：`SHOUXIN_MSVC_ROOT`（MSVC 工具集目录）、`SHOUXIN_SDK_VERSION`（Windows SDK 版本）。
> 完整说明见 [docs/BUILD.md](docs/BUILD.md)。

### 5. 启动

```bash
# 默认读取 ./config.toml，监听 127.0.0.1:3000
./target/release/shouxin-server

# 改地址 / 换配置
./target/release/shouxin-server --host 0.0.0.0:3000
SHOUXIN_CONFIG=./config.3b.toml ./target/release/shouxin-server
```

启动成功的关键日志（三条都在才算真正就绪，模型加载约需 20–60 秒）：

```
[GGUF] 初始化: GPU=false, n_gpu_layers=0, context=4096, threads=16
GGUF model loaded successfully
Embedder initialized (backend: fastembed, model: intfloat/multilingual-e5-base)
Server listening on 127.0.0.1:3000
```

### 6. 验证（4 步，全部通过即部署成功）

```bash
# ① 健康检查
curl http://127.0.0.1:3000/health

# ② 写入一段对话（立即返回 conversation_id）
curl -X POST http://127.0.0.1:3000/api/memory/add \
  -H "Content-Type: application/json" \
  -d '{"conversation":"下周三和张总开会讨论预算，下周五前给出方案。","user_id":"default_user"}'

# ③ 查抽取状态（processing → completed，3B 模型约 30–60 秒）
curl http://127.0.0.1:3000/api/memory/extract_status/<conversation_id>

# ④ 检索结果（联系人 / 待办）
curl -X POST http://127.0.0.1:3000/api/memory/search/contacts \
  -H "Content-Type: application/json" -d '{"query":"张","limit":5,"user_id":"default_user"}'
curl http://127.0.0.1:3000/api/todos
```

也可以直接跑全接口冒烟（覆盖 39 项检查）：

```bash
BASE=http://127.0.0.1:3000 USER_ID=smoke bash scripts/api_smoke_test.sh
# 期望：PASS=39  FAIL=0  SKIP=1（SKIP 为「无待确认合并」，属正常）
```

### 7. 部署自检清单

- [ ] `bash scripts/check_env.sh` 没有 `[FAIL]`
- [ ] `models/` 下有推理模型，`.fastembed_cache/` 下有嵌入模型
- [ ] 构建产物存在：`target/release/shouxin-server`（Windows 为 `.exe`）
- [ ] 启动日志出现三条就绪信息，`/health` 返回 200
- [ ] 第 6 节 4 步验证全部有结果
- [ ] `scripts/api_smoke_test.sh` 全绿

---

## 配置

配置文件默认 `./config.toml`。**支持三种推理后端**（内置 llama.cpp / Ollama / OpenAI 兼容 API），
**上下文长度可在配置里自由调节**，完整说明见 **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)**。

> 云端 API（`primary = "api"`）主链路代码已完整实现，但**尚未做端到端验证**，且心跳侧暂不支持密钥，
> 详见 [CONFIGURATION.md §2.3](docs/CONFIGURATION.md)（文中以「标注」形式列出两条已知限制）。

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
primary      = "gguf"    # gguf | ollama | api
max_tokens   = 1024
timeout_secs = 1800      # 抽取链路整轮超时（纯 CPU 时别开太小）

[llm.gguf]
model_path   = "./models/gemma-4-E4B-it-Q4_K_M.gguf"
context_size = 4096
threads      = 0         # 0 = 自动：按 [runtime].max_cpu_percent 取预算（32 核 × 60% ≈ 19）
batch_size   = 2048      # prompt 分块预填充的块大小
use_gpu      = false     # 默认纯 CPU（内存驻留，不卸载到 GPU）
n_gpu_layers = 0

[runtime]
max_cpu_percent = 60     # 整个进程占整机 CPU 上限（不是"线程数占核数"）

[embedder]
model_path       = ".fastembed_cache"
model            = "intfloat/multilingual-e5-base"
backend          = "fastembed"   # fastembed | hash
fallback_to_hash = true          # 模型缺失时降级，不阻塞启动

[server]
host = "127.0.0.1"  # 默认只监听本机；--host 优先级更高
port = 3000
```

> **`threads = 0` 表示自动**：实际线程数 = `整机逻辑核数 × max_cpu_percent%`（32 核 × 60% ≈ 19）。
> 显式填数字则作为**上限**，不会被抬高——想手动压到 8 线程就填 8。
> 更多配置项见 `config.toml` 内注释与 [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)。

### 资源占用原则（重要）

守心是**后台服务**，必须与其它进程共存，因此对资源有两条硬约束：

1. **默认纯 CPU，不占显存**
   `use_gpu = false` 是默认值（配置缺省、代码缺省都是 false）。
   模型全部驻留内存，不向 GPU 卸载任何层；`n_gpu_layers` 强制为 0。
   需要 GPU 时必须显式改 `use_gpu = true` 并用 `--features gguf-cuda` 编译。

2. **整个进程占整机 CPU 不超过 60%**
   约束对象是"**这个项目**能用多少 CPU"，不是"线程数占核数多少"——后者只是代理指标，
   而且 API / 嵌入 / 心跳的线程都不在 llama.cpp 线程数里。

   - 第一道闸门：Windows **作业对象 CPU 速率硬上限**（`max_cpu_percent`，默认 60），
     由内核按时间片结算，覆盖进程内所有线程；
   - 第二道闸门：推理与嵌入线程数 = **整机逻辑核数 × max_cpu_percent%**
     （32 逻辑核 × 60% → 19 线程），非 Windows 平台全靠这一层兜底。

   启动日志：`[CPU] 进程级硬上限已生效: 60%（整机 32 逻辑核 / 16 物理核，线程预算 19）`，
   紧接着 `[GGUF] 初始化: ... threads=19（逻辑核 32 / 物理核 16，CPU 上限 60%，预计峰值约 59%）`。

   > 百分比乘的是**逻辑核**。曾经乘在物理核上（16 × 60% = 9 线程），
   > 结果整机只能吃到 28%，看起来像"模型没在推理"——见 CHANGELOG 0.7.5。

   实测（13MB 文本嵌入压测，5 秒窗口）：约束前峰值 **94.2%**，约束后均值 **60.4%**。
   注意 Windows 的速率控制按时间片结算，2 秒窗口仍可能瞬时到 65%；
   要严格不越线，把 `max_cpu_percent` 调到 55 留出余量。

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
bash scripts/bench_cpu.sh models/Ministral-3-3B-Reasoning-2512-Q8_0.gguf
```

注意：**不要在服务运行中跑基准**，两者抢 CPU 会让数字严重失真。
方法论与踩坑记录见 **[docs/PERFORMANCE.md](docs/PERFORMANCE.md)**。

---

## API 一览

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/health` | 健康检查 |
| POST | `/api/memory/add` | 写入一段对话（立即返回 `conversation_id`；支持 `messages` 多轮与历史 `timestamp`） |
| POST | `/api/memory/batch` | 批量导入（≤500 条，逐条返回 id） |
| POST | `/api/backup` · `GET /api/export` | 一致性快照（VACUUM INTO）/ 全量 JSON 导出 |
| GET | `/api/dictionary/days` · `/day/:date` · `/conversation/:id` | 原文按会话日分页 / 当天列表 / 单条原文 |
| GET | `/api/dictionary/stats` · `/paging` | 原文库统计 / 翻页游标 |
| GET | `/api/memory/extract_status/:id` | 查询抽取进度与结果 |
| POST | `/api/memory/replay/:id` | 重放单条对话的抽取 |
| POST | `/api/memory/replay_failed` | 批量重放失败的对话 |
| POST | `/api/memory/search/semantic` | 语义检索（向量） |
| POST | `/api/memory/search/events` | 事件检索 |
| POST | `/api/memory/search/contacts` | 联系人检索（人设 / 别名 / 关系 / `disambiguation` / `is_virtual`） |
| GET | `/api/contacts/:id/detail` | **联系人全息详情**：人设 + 关联联系人 + 关联事件 + 相关记忆原文 |
| POST | `/api/memory/search/history` | 历史对话检索 |
| POST | `/api/search/smart` | 多路融合检索 |
| POST | `/api/vector/search` · `GET /api/vector/:id` | 向量库直查 / 按 id 取向量 |
| GET/POST | `/api/todos` | 待办列表 / 新建 |
| PUT | `/api/todos/:id/complete` · `DELETE /api/todos/:id` | 完成 / 删除待办 |
| GET | `/api/events/pending_merges` | 待确认的事件合并 |
| POST | `/api/events/pending_merges/:id/resolve` · `/api/events/confirm_merge` | 确认合并（merge / create / dismiss） |
| POST | `/api/files/upload` | 上传文件并入索引 |
| GET | `/api/files/:id` · `/api/files/:id/text` | 文件元数据 / 提取的文本 |
| GET | `/api/heartbeat/status` · `/api/heartbeat/pending_questions` | 心跳状态 / 待确认问题 |
| POST | `/api/heartbeat/trigger` · `/api/heartbeat/answer` | 手动触发心跳 / 回答问题 |
| GET/POST | `/api/virtual_entities` | 虚拟角色列表 / 新建（独立表） |
| GET/PUT/DELETE | `/api/virtual_entities/:id` | 角色详情（含原著设定）/ 修改 / 删除 |
| GET | `/api/virtual_entities/check/:name` | 该名字是否已被某个角色占用 |

> **联系人的身份是 `contact_id`，不是姓名。** 同名（两个"刘洋"）是允许的：
> 检索会返回多条，用 `contact_id` 引用、用 `disambiguation`（如"前端"/"后端"）展示区分。
> 规则（同名怎么判定"同一个人"）详见 [docs/API.md](docs/API.md) 的「同名联系人」一节。

完整请求/响应示例见 **[docs/API.md](docs/API.md)**。

---

## 测试

```bash
# 单元测试：109 项，不加载模型，秒级
cargo test --features gguf --lib

# 集成测试：13 个测试文件（会加载真实 ONNX 嵌入模型）
cargo test --features gguf --tests

# 全量 API 冒烟：39 项，覆盖 routes.rs 注册的全部端点（需先启动服务）
BASE=http://127.0.0.1:3000 USER_ID=smoke bash scripts/api_smoke_test.sh

# 联系人 / 虚拟角色专项验收：23 项（热冷回退、同名并存、角色隔离）
python scripts/api_acceptance.py --base http://127.0.0.1:3000 --user acc --db ./data/shouxin.db

# 联系人画像审计：逐条核对 画像 JSON / 社会关系 / 关联记忆 是否自洽
python scripts/audit_contacts.py --base http://127.0.0.1:3000 --user acc --out audit.json

# 自然语言召回评估（两套保留集，问法不含原句关键词；需先启动服务）
python scripts/eval_recall.py     # 生活/人情 12 问，目标 ≥12/12
python scripts/eval_recall2.py    # 家庭/出行 8 问，目标 8/8

# 200+ 条验收压测（自造语料：长/短/公式/人物关系/边界/同名）
python scripts/stress_200.py submit --base http://127.0.0.1:3000 --user stress200
python scripts/stress_200.py poll   --base http://127.0.0.1:3000 --user stress200
python scripts/stress_200.py verify --base http://127.0.0.1:3000 --user stress200

# 批量生成联系人画像（心跳每轮只处理 3 个，需要多轮驱动）
python scripts/gen_personas.py --base http://127.0.0.1:3000 --user stress200 --max-rounds 40
```

压测方法与暴露的缺陷见 **[docs/STRESS_TEST.md](docs/STRESS_TEST.md)**。

---

## 项目结构

```
ShouXin/
├── src/
│   ├── api/              # HTTP 路由、请求/响应结构、同步与异步入口
│   ├── config/           # 配置加载与结构定义
│   ├── core/             # 领域模型（事件/联系人/待办/记忆/事实）
│   ├── embedder/         # 嵌入器（fastembed ONNX / 离线哈希）
│   ├── heartbeat/        # 心跳（巡检、画像生成、关系分析、歧义检测）
│   ├── intent/           # 意图识别
│   ├── learning/         # 学习素材
│   ├── llm/              # LLM 客户端（gguf / ollama / api + 路由 + 并发控制）
│   ├── memory/           # 三库编排、写入器、联系人详情的联查
│   ├── memory_extractor/ # 抽取器（提示词、解析、人名过滤、正则降级）
│   ├── service/          # 后台队列、心跳调度、文件入库、内部 API
│   ├── stores/           # 三库实现（relational / vector / dictionary）
│   ├── traits/           # 抽象接口
│   ├── utils/            # 时间解析、分词、工具函数
│   └── shouxin-server.rs # 服务入口（启动、依赖装配、内置角色播种）
├── tests/                # 集成测试（10 个）
├── scripts/              # 环境自检 / 构建 / 模型下载 / 验收 / 压测 / 基准
├── docs/                 # 文档
├── migrations/           # SQLite 迁移
├── config.toml           # 默认配置（含全部注释）
├── config.3b.toml        # 低配 3B 档
└── config.toml.example   # 配置模板
```

---

## 已知限制

1. **多模态未启用**：`mmproj-BF16.gguf` 视觉塔已预留但不加载，抽取链路是纯文本。
1.1 **"知识/learning"维度未实现**：抽取提示词只要求人物/事件/待办，`learnings` 恒为空；
   只有正则降级路径会产出。早期文档把它算作第四维，实际是三维。
2. **抽取吞吐受算力限制**：4B 模型纯 CPU 单条约 30–60 秒，队列单并发，约 **2.8 条/分钟**。
   大批量导入请先灌入再等排空；追求速度可换更小模型或走 API 后端。
3. **抽取与心跳抢同一条推理通道**：心跳一轮会占住模型，导入期间建议临时调大
   `[heartbeat] base_interval_secs`，导完再调回。
4. **待办覆盖率取决于模型**：10 条生活语料实测只产出 1 条待办，模型偏保守；
   重要事项建议在对话里写清"截止/要做什么"。
5. **一句话里多个时间表达时取哪个**：现在的策略是——有"X 前 / 之前 / 最后期限 / 截止"
   这类截止标记时取标记前的那个；句中出现"最后期限/到期"等词时取**最后**一个时间表达；
   其余情况交给模型判断并与本地解析交叉校验。仍不是通用规则，
   "今天……我答应下周一……"这种没有截止词的并列句，取哪个取决于模型。
   时间语义细节见 CHANGELOG 0.7.4（含日期算术按**本地时区**计算的修正）。
6. **事件合并需人工确认**：相似度 0.90–0.96 的事件进 `pending_merges`；到期（48h）标记
   `expired`，不再自动入库，需要时得显式 resolve。
7. **推理队列无背压**：入队是无界通道，持续高速灌入时内存会增长（单条 1–2 KB 量级），
   极端场景需要外部限速。
8. **日志会记录完整提示词**（INFO 级，含对话原文）：便于排障，但日志文件需按敏感数据处理。
9. **同名信息不足时按最近互动归并**：只说"刘洋今天请假"且库里已有多个同名者时，
   会归到最近互动的那一个——信息不足时的保守选择，不会乱建号。
10. **首次构建较慢**：要编译整个 llama.cpp（Windows 约 5–10 分钟，之后增量 1 分钟内）。

## 文档

| 文档 | 内容 |
|------|------|
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | **配置详解**：gguf/Ollama/API 三种后端切换、上下文调节、阈值校准 |
| [docs/BUILD.md](docs/BUILD.md) | **构建指南**：工具链从哪来、Windows 一键构建、常见编译错误 |
| [docs/MODEL_SETUP.md](docs/MODEL_SETUP.md) | 推理模型 / 嵌入模型下载、校验与目录布局 |
| [docs/API.md](docs/API.md) | 接口详解（含同名联系人与虚拟角色的规则） |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构与三库设计、联系人多字段匹配 |
| [docs/STRESS_TEST.md](docs/STRESS_TEST.md) | **压测报告**：语料设计、检查项结论、暴露的缺陷 |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | CPU 推理性能：基准方法、根因分析、调优 |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | 排障手册 |
| [CHANGELOG.md](CHANGELOG.md) | 版本变更记录 |

---

## 版本

当前版本 **0.7.6**。变更记录见 [CHANGELOG.md](CHANGELOG.md)，发布包见 [Releases](https://github.com/liuda1999/shouxin/releases)。

---

## 许可

本项目采用 **[PolyForm Noncommercial License 1.0.0](LICENSE)**：

- ✅ 允许：个人学习、研究、实验、业余项目，以及慈善 / 教育 / 公共研究 / 政府等非商业机构使用；
- ❌ **禁止任何商业用途**，包括但不限于商业产品集成、对外收费服务、企业内部商业运营；
- 分发或修改后再分发时，必须附带本许可证与 `Required Notice` 声明。

需要商业授权请另行联系作者。完整条款见 [LICENSE](LICENSE)。

模型与依赖的许可各自独立：`gemma-4-E4B-it` 遵循 Google Gemma Terms of Use，
`Ministral-3-3B` 遵循上游 Mistral 许可，`multilingual-e5-base` 为 MIT。
