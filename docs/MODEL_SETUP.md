# 模型准备（Model Setup）

守心**不含任何模型文件**（单文件 5GB 级，不适合放进 Git 仓库）。首次部署必须手动准备两类模型：

| 类别 | 用途 | 是否必需 | 大小 |
|------|------|----------|------|
| **推理模型**（GGUF） | 从对话里抽取人物/事件/待办/知识 | 必需 | ≈4.6 GB |
| **嵌入模型**（ONNX） | 语义检索、记忆/联系人的向量化 | 必需 | ≈1.1 GB |
| **视觉塔**（mmproj GGUF） | 多模态预留，当前版本不启用 | 可选 | ≈0.95 GB |

两类模型都放在仓库根目录下的 `models/` 与 `.fastembed_cache/`。这两个目录已被 `.gitignore` 排除。

---

## 1. 推理模型（GGUF）

### 1.1 推荐模型

| 项 | 值 |
|----|-----|
| 仓库 | [`unsloth/gemma-4-E4B-it-GGUF`](https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF) |
| 量化 | `Q4_K_M` |
| 文件 | `gemma-4-E4B-it-Q4_K_M.gguf`（≈4.6 GB） |
| 视觉塔（可选） | `mmproj-BF16.gguf`（≈0.95 GB） |
| 许可证 | 遵循上游 Gemma 许可，商用前请自行确认 |

### 1.2 下载

**方式 A：直接下载（有国际网络）**

```bash
mkdir -p models
curl -L -o models/gemma-4-E4B-it-Q4_K_M.gguf \
  "https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf"

# 可选：视觉塔
curl -L -o models/mmproj-BF16.gguf \
  "https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF/resolve/main/mmproj-BF16.gguf"
```

**方式 B：国内镜像 + 分段并行下载（推荐）**

直连 huggingface.co 或走 `hf-mirror.com` 时，单连接常被限速到几十 KB/s（5GB 要下几十小时）。
仓库内置分段并行下载脚本，实测可把带宽拉满到数 MB/s：

```bash
python scripts/download_gguf.py \
  "https://hf-mirror.com/unsloth/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf" \
  "models/gemma-4-E4B-it-Q4_K_M.gguf" 32
```

参数说明：第 1 个是 URL，第 2 个是输出文件，第 3 个是并发分段数（默认 24，网络好可加到 32~64）。

脚本特性：

- 先 `HEAD` 取 `Content-Length`，把文件切成 N 段并行拉取；
- 每段独立重试，中断后重跑可续传（已完成的分段会跳过）；
- 结束时校验文件长度，长度不符会以非 0 退出，不会静默产出损坏文件。

> 脚本默认关闭 TLS 证书校验（`GGUF_TLS_VERIFY=0`），因为本地代理常做 TLS 中间人导致握手失败。
> 在可信网络下建议 `GGUF_TLS_VERIFY=1` 打开校验。

### 1.3 校验

```bash
ls -l models/gemma-4-E4B-it-Q4_K_M.gguf     # 应为 4.6 GB 左右
xxd -l 4 models/gemma-4-E4B-it-Q4_K_M.gguf  # 应以 GGUF (47 47 55 46) 开头
```

### 1.4 兼容性说明

- **架构支持**：`llama.cpp` 需支持 `LLM_ARCH_GEMMA4`。本项目依赖的 `llama-cpp-sys-2 = 0.1.160`
  已包含该架构（`grep LLM_ARCH_GEMMA4 src/llama-arch.h` 可自证）。
- **换更小/更大的模型**：只要 `llama.cpp` 支持其架构，改 `config.toml` 里的 `[llm.gguf].model_path`
  即可。3B 级（Q4/Q5）适合纯 CPU 推理；7B 以上纯 CPU 会明显变慢。

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

`mmproj-BF16.gguf` 是 Gemma 多模态的视觉投影层。**当前版本（v0.5.0）的抽取链路是纯文本的，
不会加载 mmproj**，下载它只是为了后续多模态能力预留。不下载不影响任何现有功能。

```bash
curl -L -o models/mmproj-BF16.gguf \
  "https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF/resolve/main/mmproj-BF16.gguf"
```

---

## 4. 目录总览（模型就绪后的样子）

```
ShouXin/
├── models/
│   ├── gemma-4-E4B-it-Q4_K_M.gguf      # 推理模型 4.6 GB
│   └── mmproj-BF16.gguf                # 可选 0.95 GB
├── .fastembed_cache/
│   └── intfloat/multilingual-e5-base/  # 嵌入模型 1.1 GB
└── config.toml
```

## 5. 一键检查

```bash
bash scripts/check_env.sh
```

会检查：Rust 工具链、模型文件、嵌入缓存、端口占用、配置项一致性，并给出缺失项的下载命令。
