# 变更记录

## [未发布] 2026-10-29 — 资源占用硬约束：默认不占显存 + GPU 模式 CPU ≤ 60%

背景：守心是**后台服务**，必须与其它进程共存。显存是稀缺资源，CPU 也不能被吃满。

### 变更

| 项 | 变化 |
|----|------|
| **默认纯 CPU** | `use_gpu` 的**代码缺省值从 `true` 改为 `false`**（配置缺省与 `GgufClient` 内部缺省一致），`n_gpu_layers` 恒为 0。不显式打开就绝不占显存 |
| **新增 `gpu_max_cpu_percent`** | `[llm.gguf]` 新配置项，默认 **60**。GPU 模式实际线程数 = `min(threads, **物理核数** × 百分比%)`，向下取整、最少 1 |
| 纯 CPU 模式 | 线程数沿用 `threads`，且不超过核心数 |
| 心跳客户端 | 固定纯 CPU（`use_gpu=false`），不占显存 |

### 实现

```rust
// src/llm/gguf.rs
pub(crate) fn resolve_threads(configured, cores, gpu_enabled, percent) -> usize {
    let cores = cores.max(1);
    if gpu_enabled { cap_threads_for_gpu(configured, cores, percent) }
    else { configured.max(1).min(cores) }
}
```

- 基准取**物理核心数**（`num_cpus::get_physical()`），不是逻辑核 ——
  否则 32 线程（16 物理核 + 超线程）× 60% = 19 会超过物理核数，反而造成超载；
- GPU 模式被压制时打日志：`[GGUF] GPU 模式限制 CPU 占用：threads 32 -> 19（核心 32，上限 60%）`；
- 启动日志新增核心数与上限信息，一眼可核对：
  `[GGUF] 初始化: GPU=false, n_gpu_layers=0, context=4096, threads=16（物理核 16，GPU 上限 60%）`；
- **未编译 CUDA 却写了 `use_gpu=true`** 时明确告警并回退纯 CPU（避免"以为用了 GPU、实际既没加速也没省事"）：
  `配置要求 GPU 推理（use_gpu=true），但当前二进制未编译 CUDA 后端…本次将回退纯 CPU 运行`。

### 测试

- 新增 5 个单元测试（`thread_cap_tests`）：60% 折算、配置低于上限时不干预、
  下限为 1、百分比越界夹取、CPU 分支不叠加限制；
- `cargo test --features gguf --lib`：**74 passed**（原 69 + 5）；
- `cargo test --features gguf --tests`：**91 passed**；
- `bash scripts/api_smoke_test.sh`：**PASS=35 FAIL=0 SKIP=0**（Ministral 3B 配置实跑）。

GPU 模式的线程压制已在运行时实测（用 `use_gpu=true` 的临时配置启动）：

```
[GGUF] GPU 模式限制 CPU 占用：threads 16 -> 9（核心 16，上限 60%）
[GGUF] 初始化: GPU=false, n_gpu_layers=0, threads=9（物理核 16，GPU 上限 60%）
[GGUF] 配置要求 GPU 推理（use_gpu=true），但当前二进制未编译 CUDA 后端…回退纯 CPU 运行
```

即：16 物理核 → 最多 9 线程（≤60%），且因为本机是纯 CPU 构建，显存占用为 0。

---

## [未发布] 2026-10-29 — 推理模型换挡：下线 Llama-3.2，新增 Ministral-3B 低配档

### 模型

| 项 | 变化 |
|----|------|
| 退役 | `Llama-3.2-3B-Instruct-Q5_K_M` **不再使用**，配置/脚本/文档中的引用全部移除 |
| 新增低配档 | `Ministral-3-3B-Reasoning-2512-Q8_0.gguf`（3.4 GB）+ `mmproj-BF16.gguf`（0.78 GB） |
| 标准档 | 不变，仍为 `gemma-4-E4B-it-Q4_K_M`（默认 `config.toml`） |
| 新增配置 | `config.3b.toml` —— 指向 Ministral，`SHOUXIN_CONFIG=./config.3b.toml` 即可切档 |
| 架构兼容 | Ministral 为 `mistral3`，`llama-cpp-sys-2 0.1.146` 已支持；对话模板从 GGUF 正常读取（`[INST]`） |

实测（16 线程 / ctx 4096 / 纯 CPU / 服务未运行）：

| 模型 | 生成速度 | 单次抽取端到端 |
|------|----------|----------------|
| Gemma-4-E4B Q4_K_M | ≈ 16 tok/s | ≈ 33 s |
| Ministral-3-3B Q8_0 | ≈ 15.4 tok/s | ≈ 41 s |

### 新增

- `scripts/api_smoke_test.sh`：**全量 API 冒烟测试**，覆盖 `routes.rs` 注册的全部 35 个端点
  （记忆写入/状态/重放、四类检索、待办 CRUD、事件合并、文件上传与越权、心跳、
  虚拟角色 CRUD、向量检索），自带数据隔离与结果汇总。
  当前基线：`PASS=35 FAIL=0 SKIP=0`，Gemma 与 Ministral 两套配置均通过。
- `config.3b.toml`：低配 3B 档配置模板。

### 修正

- `scripts/check_env.sh` 支持 `SHOUXIN_CONFIG`，可校验非默认配置（如 `config.3b.toml`）；
- 文档中"跨 owner 访问返回 404"更正为**实际行为**：HTTP 200 + `{"success":false,...}`
  （不泄露资源是否存在）；
- 明确记录一个测量陷阱：**服务运行中跑基准会因 CPU 争抢导致数字严重失真**
  （64 token 从 5 s 恶化到超时），已在 PERFORMANCE.md 标注。

---

## [未发布] 2026-10-29 — 联系人画像与关系自动生成（专项）

针对"联系人的人设/别名/社会关系/联系人之间关系能否自动生成"做了完整排查，
发现链路有 6 处断点，全部修复并端到端验证。

| 编号 | 问题 | 影响 |
|------|------|------|
| C1 | `search_contacts` 构造 `ContactDetail` 时把 `aliases/background/related_contacts/relationship_tags` **硬编码为空** | 即使库里有画像，接口也只返回空值 —— 看起来像"从没生成过" |
| C2 | 别名分两处存（`contact_index.aliases` 与 `contact_alias_index`），读取只看前者 | 抽取器写入的昵称（"王建国"）在接口里看不到 |
| C3 | `build_chat_prompt` 在模型句柄加载**之前**执行，首次调用拿不到 GGUF 模板 | 心跳画像用错模板 → 模型复述提示词 → 画像 JSON 永远解析失败 |
| C4 | 画像提示词要求"全部输出中文"，模型把 **JSON key 也翻成中文** | 解析器只认英文 key，字段全空 |
| C5 | 画像只喂"事件摘要"，不喂对话原文；且召回用的 `get_conversations_since_v2` **只返回 pending/failed** | 模型看不到"老王是我的同事"，只能输出"未知" |
| C6 | 画像里的 `social_relations` 只写进 `background` 的 JSON，`relationship_tags` / `related_contact_ids` 两列从未更新 | 联系人之间的关系永远是 `[]` |
| C7 | 抽取提示词的 few-shot 示例（李总/赵总/王总监…）被模型照抄成联系人 | 联系人列表被"李总的女儿""张总的秘书"等**虚构人名**污染 |

修复要点：

- 读取路径真正读库，并合并两处别名来源；关联联系人 ID 批量解析成姓名
- `build_chat_prompt` 前先 `ensure_loaded()`，杜绝"首次调用用错模板"
- 画像提示词：key 保持英文、value 用中文，并给出结构示例；解析器兼容数组/对象/中文 key 三种形态
- 画像证据补齐：新增 `get_recent_conversations_for_owner()`（不区分抽取状态），
  把"含该联系人或其别名"的最近 3 段原文喂进提示词
- 画像落库时同步写 `relationship_tags` 与 `related_contact_ids`，
  并把 `related_contact_name` 在同一 owner 内解析成 ID（`find_contact_id_by_name_same_owner`）
- 联系人过滤新增规则 8：**姓名或其别名必须逐字出现在对话原文中**，否则丢弃

端到端实测（用户 `rel4`，对话含"老王是我同事／小李与老王同组／张姐是上级"）：

```
张姐  aliases=[]        tags=['上级']  related=['老王','小李']  画像 505 字符
小李  aliases=[]        tags=['同事']  related=['老王','张姐']  画像 680 字符
老王  aliases=['王建国'] tags=['同事']  related=['小李']        画像 579 字符
```

> 生成机制与频率：由**心跳**驱动（`analyze_contact_relations`），每轮最多处理 3 个"画像为空"的联系人，
> 单个联系人每天最多更新 2 次、间隔 ≥ 8 小时（`MAX_PROFILE_UPDATES_PER_DAY` / `PROFILE_UPDATE_INTERVAL_HOURS`）。
> 想立即看到效果可 `POST /api/heartbeat/trigger {"maintenance_only": false}`。

---

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
