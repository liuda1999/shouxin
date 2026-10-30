# 模型准备（Model Setup）

守心**不含任何模型文件**（单文件 3–5 GB 级，不适合放进 Git 仓库）。首次部署必须手动准备：

| 类别 | 用途 | 是否必需 | 体积 |
|------|------|----------|------|
| **推理模型**（GGUF） | 从对话里抽取人物/事件/待办/知识 | 必需（二选一） | 3.4 ~ 4.6 GB |
| **嵌入模型**（ONNX） | 语义检索、记忆/联系人的向量化 | 必需 | ≈1.1 GB |
| **视觉塔**（mmproj GGUF） | 多模态预留，当前版本不加载 | 可选 | ≈0.8 ~ 0.95 GB |

推理模型有两档，按机器配置二选一：

| 档位 | 模型 | 体积 | 启动方式 |
|------|------|------|----------|
| **标准（默认）** | [`unsloth/gemma-4-E4B-it-GGUF`](https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF) `Q4_K_M` | 4.6 GB | `./shouxin-server`（默认 `config.toml`） |
| **低配 3B** | [`unsloth/Ministral-3-3B-Reasoning-2512-GGUF`](https://huggingface.co/unsloth/Ministral-3-3B-Reasoning-2512-GGUF) `Q8_0` | 3.4 GB | `SHOUXIN_CONFIG=./config.3b.toml ./shouxin-server` |

两者都在纯 CPU 上跑：16 线程下约 **15–16 tok/s**，单次抽取 30–45 秒。

---

## 1. 推理模型（GGUF）

### 1.1 标准档（默认）：Gemma-4-E4B-it Q4_K_M

| 项 | 值 |
|----|-----|
| 仓库 | https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF |
| 镜像 | https://hf-mirror.com/unsloth/gemma-4-E4B-it-GGUF |
| 文件 | `gemma-4-E4B-it-Q4_K_M.gguf`（≈4.6 GB） |
| 视觉塔（可选） | `mmproj-BF16.gguf`（≈0.95 GB） |
| 许可证 | 遵循上游 Gemma 许可，商用前请自行确认 |

```bash
python scripts/download_gguf.py \
  "https://hf-mirror.com/unsloth/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf" \
  "models/gemma-4-E4B-it-Q4_K_M.gguf" 32
```

### 1.2 低配 3B 档：Ministral-3-3B-Reasoning-2512 Q8_0

| 项 | 值 |
|----|-----|
| 仓库 | https://huggingface.co/unsloth/Ministral-3-3B-Reasoning-2512-GGUF |
| 文件 | `Ministral-3-3B-Reasoning-2512-Q8_0.gguf`（≈3.4 GB） |
| 视觉塔（可选） | `mmproj-BF16.gguf`（≈0.78 GB） |
| 架构 | `mistral3`（内置 llama.cpp 已支持） |

```bash
# 直连（有国际网络）
python scripts/download_gguf.py \
  "https://hf-mirror.com/unsloth/Ministral-3-3B-Reasoning-2512-GGUF/resolve/main/Ministral-3-3B-Reasoning-2512-Q8_0.gguf" \
  "models/Ministral-3-3B-Reasoning-2512-Q8_0.gguf" 32
```

镜像地址（浏览器直接下载）：

- 模型：`https://hf-mirror.com/unsloth/Ministral-3-3B-Reasoning-2512-GGUF/resolve/main/Ministral-3-3B-Reasoning-2512-Q8_0.gguf?download=true`
- 视觉塔：`https://hf-mirror.com/unsloth/Ministral-3-3B-Reasoning-2512-GGUF/resolve/main/mmproj-BF16.gguf?download=true`

启用低配档：仓库已带 `config.3b.toml`，只需换配置：

```bash
SHOUXIN_CONFIG=./config.3b.toml ./target/release/shouxin-server
```

想用它做默认，也可以直接改 `config.toml` 里的两处路径：

```toml
[llm.gguf]
model_path = "./models/Ministral-3-3B-Reasoning-2512-Q8_0.gguf"

[llm.heartbeat]
model = "./models/Ministral-3-3B-Reasoning-2512-Q8_0.gguf"
```

### 1.3 下载脚本说明（`scripts/download_gguf.py`）

国内直连 huggingface.co / hf-mirror 时，**单连接常被限速到几十 KB/s，且长连接会被中途掐断**，
普通 `curl` 会静默产出截断文件（GGUF 头正常，加载时才报错）。脚本做了三件事：

1. 先 `HEAD` 取 `Content-Length`，把文件切成 N 段并行拉取（默认 24，网络好可加到 32~64）；
2. 每段按"已下载字节数"**续传**，中断后重跑可继续；
3. 结束校验最终文件长度 == 服务端长度，不符则以非 0 退出。

```bash
python scripts/download_gguf.py <url> <输出文件> [并发数]
```

> 脚本默认关闭 TLS 证书校验（`GGUF_TLS_VERIFY=0`），因为本地代理常做 TLS 中间人导致握手失败。
> 可信网络下建议 `GGUF_TLS_VERIFY=1` 打开校验。

### 1.4 校验

```bash
ls -l models/*.gguf                                   # 体积应与上表一致
xxd -l 4 models/Ministral-3-3B-Reasoning-2512-Q8_0.gguf   # 应为 GGUF
bash scripts/check_env.sh                             # 会自动校验当前配置指向的模型
```

### 1.5 兼容性：模型不写死

**本项目不绑定任何具体模型。** 代码不认模型名、不校验文件哈希、不假设词表大小，只做三件事：
按配置读 GGUF 路径、交给 `llama.cpp` 加载、用模型自带的停止符判停。因此判断标准只有一条：

> **你的 GGUF 架构在 `llama.cpp` 的架构表里，就能用。**

项目依赖 `llama-cpp-sys-2 = 0.1.146`，其内置 `LLM_ARCH_NAMES` 共 **126 个架构**，涵盖
llama / llama4 / qwen / qwen2 / qwen3 / qwen35 / phi2 / phi3 / gemma–gemma4 / mistral3 / mistral4 /
glm4 / deepseek2 / granite / olmo2 / smollm3 / ernie4_5 / minimax-m2 等。
自证方法：`grep -c "{ LLM_ARCH_" ~/.cargo/registry/src/*/llama-cpp-sys-2-0.1.146/llama.cpp/src/llama-arch.cpp`。

**换模型的完整清单（三步）**：

| 步骤 | 位置 | 说明 |
|---|---|---|
| ① 改对话抽取模型 | `[llm.gguf].model_path` | 主链路，抽取人物/事件/待办/知识 |
| ② 改心跳模型 | `[llm.heartbeat].model` | 画像与维护**独立配置**；漏改会继续加载旧模型，旧文件删了就报"模型文件不存在"，画像全线停摆 |
| ③ 按需调上下文 | `[llm.gguf].context_size` | 默认 4096；长上下文模型不改这项等于白搭 |

**需要预期的一次调试**：对话模板。Gemma（`<start_of_turn>`）与 Qwen（`<|im_start|>`）由代码内置；
其余家族优先用 GGUF 自带的 Jinja 模板；Jinja 渲染失败才退回 `[INST] … [/INST]`（Mistral 系正确，
Llama-3 / Phi / GLM 这类若走到该兜底，模型可能复述输入而非输出 JSON，此时用
`SHOUXIN_SKIP_CHAT_TEMPLATE=1` 反向排查）。量化格式任意（Q4/Q5/Q8 都行），只影响内存与速度：
3B–7B Q4 适合纯 CPU，7B 以上纯 CPU 会明显变慢（瓶颈是内存带宽，见 PERFORMANCE.md §1.6）。

---

## 2. 嵌入模型（ONNX）

### 2.1 推荐模型

| 项 | 值 |
|----|-----|
| 仓库 | [`intfloat/multilingual-e5-base`](https://huggingface.co/intfloat/multilingual-e5-base) |
| 维度 | 768 |
| 语言 | 中英等 100+ 语言（中文语义检索效果明显优于英文模型） |
| 文件 | `onnx/model.onnx`（≈1.1 GB） |

### 2.2 下载与目录布局

FastEmbed 不走 HuggingFace 的 `models--org--name/snapshots/...` 布局，而是要求
`{缓存根}/<org>/<model>/onnx/model.onnx`。用内置脚本一步到位：

```bash
bash scripts/fetch_embedding_model.sh
```

脚本会从 `hf-mirror.com` 下载并落到如下结构（`FASTEMBED_CACHE_DIR` 默认 `.fastembed_cache`）：

```
.fastembed_cache/
└── intfloat/
    └── multilingual-e5-base/
        ├── config.json
        ├── tokenizer.json
        ├── tokenizer_config.json
        ├── special_tokens_map.json
        └── onnx/
            └── model.onnx          # 约 1.1 GB
```

### 2.3 配置项

`config.toml` 里嵌入器有 4 个相关项，**注意别把「缓存目录」和「模型名」搞混**（这是早期最常见的一个坑）：

```toml
[embedder]
model_path = ".fastembed_cache"            # 缓存根目录（相对仓库根）
model = "intfloat/multilingual-e5-base"    # HuggingFace 仓库 ID，不是路径
backend = "fastembed"                      # fastembed | hash
fallback_to_hash = true                    # 模型缺失时降级为离线哈希嵌入
```

### 2.4 离线 / 无模型时的降级

若模型缺失或下载失败：

- `fallback_to_hash = true`（默认）→ 服务照常启动，改用内置**离线哈希嵌入器**。
  它用字符 n-gram 哈希 + L2 归一化生成向量，语义检索可用但精度低于 e5，**适合先跑通链路**。
- `fallback_to_hash = false` → 启动直接失败。

也可以显式指定 `backend = "hash"`，完全不依赖 ONNX 运行时（体积最小、启动最快）。
启动日志会明确打出当前用的是哪条路径：

```
Embedder initialized (backend: fastembed, model: intfloat/multilingual-e5-base)
# 或
Embedder initialized (backend: hash, dimension: 768, reason: ...)
```

---

## 3. 视觉塔（mmproj，可选）

mmproj 是 Gemma / Ministral 多模态的视觉投影层。**当前版本（v0.6.x）的抽取链路是纯文本的，
不会加载 mmproj**，下载它只为后续多模态能力预留。不下载不影响任何现有功能。

```bash
# Gemma 用
python scripts/download_gguf.py \
  "https://hf-mirror.com/unsloth/gemma-4-E4B-it-GGUF/resolve/main/mmproj-BF16.gguf" \
  "models/mmproj-BF16.gguf" 16

# Ministral 用
python scripts/download_gguf.py \
  "https://hf-mirror.com/unsloth/Ministral-3-3B-Reasoning-2512-GGUF/resolve/main/mmproj-BF16.gguf" \
  "models/mmproj-Ministral-3-3B-BF16.gguf" 16
```

---

## 4. 目录总览（模型就绪后的样子）

```
ShouXin/
├── models/
│   ├── gemma-4-E4B-it-Q4_K_M.gguf              # 标准档 4.6 GB
│   ├── Ministral-3-3B-Reasoning-2512-Q8_0.gguf # 低配 3B 3.4 GB（可选）
│   ├── mmproj-BF16.gguf                        # 可选（Gemma）
│   └── mmproj-Ministral-3-3B-BF16.gguf         # 可选（Ministral）
├── .fastembed_cache/
│   └── intfloat/multilingual-e5-base/          # 嵌入模型 1.1 GB
├── config.toml                                  # 默认：Gemma
└── config.3b.toml                               # 低配：Ministral 3B
```

> 已退役的 `Llama-3.2-3B-Instruct-Q5_K_M` 若仍在本地，可放到 `models/_retired/` 保留备份；
> 项目不再引用它。

## 5. 一键检查

```bash
bash scripts/check_env.sh
```

会检查：Rust 工具链、**当前配置指向的**推理模型、嵌入缓存、端口占用、
配置一致性（`use_gpu/threads/n_gpu_layers`），以及 llama.cpp 是否真的编了 AVX2。
