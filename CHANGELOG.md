# 变更记录

## [未发布] 2026-10-28 — 启动阻塞、CPU 性能、新模型

本轮目标：让项目**能按文档从零部署并跑通**，并把 CPU 推理拉到可用水平。

### 修复（影响正确性）

| 编号 | 问题 | 影响 | 处理 |
|------|------|------|------|
| F1 | 配置把**缓存目录**当成**模型名**传给 FastEmbed | 嵌入模型永远加载失败，服务起不来 | 配置拆成 `model_path`（缓存根）与 `model`（HF 仓库 ID） |
| F2 | 嵌入模型是硬依赖 | 缺模型直接退出 | 新增 `[embedder].backend = fastembed\|hash` + `fallback_to_hash`，并实现离线哈希嵌入器 |
| F3 | 待办"模糊匹配"实为 `LIKE '%词%'` 连续子串 | "写报告"匹配不到"写项目报告"，按对话更新待办失效 | 改为子序列匹配（含单测） |
| F4 | `parse_event_time` 的"下周X"多算一周 | "下周一"算成 11-09（应为 11-02），所有事件时间受影响 | 公式修正为 `7 - since + offset` |
| F5 | 待办 deadline 直接采信模型给的日期 | 小模型常给出过期日期（"下周一"→ 07-11） | 改为优先用本地 `time_parser` 从原文解析，仅精确时间表达；模型日期退为兜底 |
| F6 | `pending_merges` 列表解析按 RFC3339 处理 SQLite 时间 | 接口报 `premature end of input`，确认入口不可用 | 新增 `parse_sqlite_datetime()` 兼容 `2026-10-28 13:38:37` 格式 |
| F7 | 提示词整体塞进一个 `n_batch` 大小的 batch | `prompt_tokens > n_batch` 时必然失败，LLM 抽取**静默降级为正则** | 改为分块预填充（chunked prefill），且只有最后一个 token 取 logits |
| F8 | 对话模板写死 Llama-3 格式 | 换 Gemma 后模型复述原文、拿不到 JSON | 优先用 GGUF 自带 `tokenizer.chat_template`，失败则按 `general.architecture` 内置 gemma / qwen / llama 模板 |
| F9 | 停止判定只比 `token_eos()` | Gemma 的 `<end_of_turn>` 不触发停止，白跑 1024 token | 改用 `is_eog_token()` |
| F10 | `n_gpu_layers = 0` 被当成"自动探测" | 宣称纯 CPU 却仍卸载 28 层到 GPU；心跳客户端还硬编码 99 层 | 新增 `[llm.gguf].use_gpu` 显式开关，心跳客户端同步 |

### 修复（影响性能）

| 编号 | 问题 | 效果 |
|------|------|------|
| P1 | 每 token 对全词表做全量构造 + 排序采样 | 改用 llama.cpp 原生采样链（top-k → top-p → temp → dist） |
| P2 | `GGML_AVX2=ON` 只写进缓存变量，没有变成 `/arch:AVX2` | 工具链文件里显式注入编译标志 |
| **P3** | **`CMAKE_SYSTEM_PROCESSOR` 为空 → ggml 走 `-DGGML_CPU_GENERIC` 标量兜底** | **生成速度 3.8 → 24 tok/s（3B）**，单次调用固定开销 8.9 s → 1.3 s |
| P4 | 构建脚本用 POSIX 路径调 Windows 版 cmake/ninja | 表现为"构建脚本静默退出 1"，改用 Windows 绝对路径 |

详见 [docs/PERFORMANCE.md](docs/PERFORMANCE.md)。

### 新增

- `src/bin/bench_gguf.rs`：纯推理基准工具（可扫线程数），配 `scripts/bench_cpu.sh`
- `scripts/download_gguf.py`：分段并行 + 断点续传下载，结束校验文件长度
- `scripts/check_env.sh`：部署前环境自检
- `scripts/dev_env.sh`：导出 MSVC/Ninja/AVX2 环境，使任意 `cargo` 命令（含 `cargo test --features gguf`）可用
- `scripts/llama_cpu_toolchain.cmake`：MSVC 下强制 AVX2 + 正确架构
- `docs/`：`MODEL_SETUP` / `BUILD` / `PERFORMANCE` / `TROUBLESHOOTING` / `API` / `ARCHITECTURE`
- `GET /api/events/pending_merges`、`POST /api/events/pending_merges/{id}/resolve`

### 变更

- 默认推理模型换为 `gemma-4-E4B-it-Q4_K_M`（4.6 GB），嵌入仍为 `multilingual-e5-base`
- `gguf` 特性改为**纯 CPU**（不再编译 CUDA 后端）；新增 `gguf-cuda` 供需要 GPU 时使用。
  副作用：二进制不再依赖 `cublas64_12.dll`，构建时间从 ~15 分钟降到 ~1 分钟
- 配置默认值：`threads=16`、`batch_size=2048`、`context_size=4096`、`timeout_secs=1800`、`use_gpu=false`

### 验证基线

- `cargo test --lib` 69 passed / 0 failed
- `cargo test --tests` 91 passed / 0 failed
- 真实语料端到端：Gemma-4-E4B @ 15.5–16 tok/s，33 s 完成 2 人 / 5 待办提取，
  deadline 全部正确（下周一=11-02、下周三=11-04、下周五=11-06）

### 已知限制

- 事件相似度 0.75–0.90 不自动合并，需确认后才进索引（可用接口确认）
- mmproj 视觉塔已下载但**不加载**（抽取链路为纯文本）
- `llama-cpp-2` 0.1.160 的 crate 包缺少 `llama.cpp/app` 目录无法编译，故仍用 0.1.146（已支持 GEMMA4）
