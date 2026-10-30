# 配置说明（Configuration）

守心的所有行为都由 `config.toml` 驱动。本页按"最常改的放前面"组织。

> 改完建议跑一次一致性检查，能发现"写了但不生效"的死配置：
>
> ```bash
> python scripts/check_config.py config.toml config.3b.toml
> ```

---

## 1. 配置从哪来（覆盖顺序）

后者覆盖前者：

| 顺序 | 来源 | 示例 |
|------|------|------|
| 1 | `config.toml` | 默认配置 |
| 2 | 同目录 `config.local.toml` | 本地私有覆盖（已在 `.gitignore`） |
| 3 | 环境变量（前缀 `SHOUXIN__`，层级用双下划线） | `SHOUXIN__SERVER__PORT=3100` |
| 4 | 命令行参数 | `--host 0.0.0.0:8080`、`--data-dir ./data_b` |

切换整套配置：

```bash
SHOUXIN_CONFIG=./config.3b.toml ./shouxin-server    # 低配 3B 档
```

数据目录（SQLite / Sled / 向量库）默认跟随 `[storage].data_dir`；用 `--data-dir` 可让**所有**数据落到指定目录，
适合多实例隔离或测试：

```bash
./shouxin-server --data-dir ./data_test --host 127.0.0.1:3011
```

---

## 2. 选择推理方式（`[llm].primary`）

守心支持三种推理后端，**可随时切换**，对上层 API 与数据格式没有影响。

### 2.1 内置 llama.cpp（默认，纯 CPU）

```toml
[llm]
primary = "gguf"
secondary = "none"

[llm.gguf]
model_path = "./models/gemma-4-E4B-it-Q4_K_M.gguf"
context_size = 4096
threads = 16
batch_size = 2048
use_gpu = false          # 后台服务原则：默认不占显存
n_gpu_layers = 0
gpu_max_cpu_percent = 60 # 仅 GPU 模式生效
offload_kv_cache = false
```

- **零外部依赖**，模型加载进内存，数据完全不出本机；
- 首次启动会加载模型（3~5 GB），之后常驻；
- `use_gpu = true` 需要二进制用 `--features gguf-cuda` 编译，否则会打 WARN 并回退纯 CPU。

### 2.2 Ollama（复用你已有的本地模型服务）

```toml
[llm]
primary = "ollama"

[llm.ollama]
model = "qwen2.5:7b"                  # 换成 `ollama list` 里的任意模型
endpoint = "http://localhost:11434"
timeout_secs = 300
max_retries = 2
```

- 适合已经有 Ollama、或想在 GPU 上跑更大模型的情况；
- 此时 `[llm.gguf]` 的模型文件可以不下载；
- 注意：Ollama 侧的上下文长度由它自己的 `num_ctx` 决定，守心的 `context_size` 不生效。

### 2.3 云端 API（OpenAI 兼容）

> **状态：已实现但未做端到端验证（标注）**
>
> 主链路代码完整（`src/llm/api.rs` 实现全部 `LlmClient` 方法），但仓库内没有任何配置档或测试真正跑过
> `primary = "api"`。启用前请先读本节末尾的两条限制。

```toml
[llm]
primary = "api"

[llm.api]
# 必须写到 /chat/completions 全路径：客户端直接 POST 此地址，不会自动补全
endpoint = "https://api.openai.com/v1/chat/completions"
api_key = ""                             # 建议用环境变量注入
model = "gpt-4o-mini"
timeout_secs = 60
max_retries = 2
```

注入密钥（不要写进仓库）：

```bash
export SHOUXIN__LLM__API__API_KEY="sk-..."
```

- 适合机器弱、或想省电省内存；
- **注意隐私边界**：这条路会把对话内容发到你配置的服务端，与"本地优先"的定位相冲突，按需选择。

**已知限制（待实现，先标注）**

1. **心跳后端暂不支持 API 密钥。** `[llm.heartbeat].backend = "api"` 时密钥被固定为空串
   （`HeartbeatLlmSettings` 没有 `api_key` 字段），只能对接无需鉴权的本地/自建网关。
2. **改主链路不会停掉本地模型。** `[llm.heartbeat].backend` 默认仍是 `gguf`，即使
   `primary = "api"`，启动仍会加载本地 gguf 模型（约 4.7 GB 常驻内存）。
   受限制 1 影响，目前无法只靠改配置彻底切到云端。

<!-- TODO(cloud-api): 实现 [llm.heartbeat].api_key + headers 透传，并补一条 primary="api" 的端到端测试 -->


### 2.4 主备切换

```toml
[llm]
primary = "gguf"
secondary = "ollama"    # 主模型失败时自动切备用；"none" 表示不启用
```

---

## 3. 上下文长度（`context_size`）怎么调

`context_size` 决定了"一次能塞进多少 token"，直接换内存和速度：

| context_size | KV 缓存内存（3B/4B 级别） | 适用场景 |
|--------------|---------------------------|----------|
| 2048 | ≈0.2–0.3 GB | 只处理短对话，最省内存 |
| **4096（默认）** | ≈0.4–0.6 GB | 常规对话 + 抽取提示（推荐） |
| 8192 | ≈0.8–1.2 GB | 长会议记录、长文档片段 |
| 16384+ | ≈1.6 GB 起 | 很少需要；纯 CPU 下会明显变慢 |

调整建议：

1. **先看实际需要**：抽取提示本身约 1–2k token，普通对话几百 token，4096 通常够用；
2. **加大前先确认内存**：每个并发上下文都独立占 KV 缓存；
3. **超长输入不必靠加大 context 硬扛**：抽取链路会自动分块（约 1200 字符/块，最多 8 块），
   原文完整保存在字典库，加大 `context_size` 只影响单次可读入的文本量；
4. 改完重启服务生效。

```toml
[llm.gguf]
context_size = 8192
```

> 相关：`batch_size`（默认 2048）是**预填充块大小**，影响长 prompt 的处理速度，一般不用动。

---

## 4. 嵌入模型（`[embedder]`）

```toml
[embedder]
model_path = ".fastembed_cache"              # 缓存根目录
model = "intfloat/multilingual-e5-base"      # HF 仓库 ID（不是路径！）
backend = "fastembed"                        # fastembed | hash
hash_dimension = 768
fallback_to_hash = true                      # 模型缺失时降级，不阻塞启动
```

- `backend = "hash"`：完全不加载 ONNX，用字符 n-gram 哈希生成向量。省内存、启动快，语义精度低；
- `fallback_to_hash = true`：模型目录缺失时自动降级，服务仍能起来；
- **换嵌入模型必须重新校准事件合并阈值**（见第 6 节），因为相似度分布随模型变化。

---

## 5. 心跳（`[heartbeat]`）

后台自维护：待办失效/归档/提醒、联系人画像、事件合并确认等。

```toml
[heartbeat]
enabled = true
base_interval_secs = 60
min_interval_secs = 30
max_interval_secs = 120
adaptive = true                 # 按空闲程度自动拉长/缩短间隔
idle_threshold_secs = 30
maintenance_timeout_secs = 30
llm_timeout_secs = 120
memory_threshold_mb = 4096      # 可用内存低于该值则跳过本轮
max_consecutive_skips = 3
```

- 想临时关掉后台 LLM 调用：`enabled = false`；
- 只想跑维护不动模型：`POST /api/heartbeat/trigger {"maintenance_only": true}`；
- 孤儿向量清理已限流为**每 6 小时一次**（需全量读向量库，不宜高频）。

---

## 6. 抽取与事件合并（`[extraction]`）

```toml
[extraction]
timeout_secs = 1800
intent_grace_secs = 300
# 相似度阈值：**与嵌入模型强相关**
event_merge_auto_threshold = 0.96
event_merge_pending_threshold = 0.90
```

含义：

| 相似度 | 行为 |
|--------|------|
| > `auto` | 直接合并到已有事件 |
| `pending` ~ `auto` | 进待确认区，等 `/api/events/pending_merges` 确认 |
| < `pending` | 作为新事件落库 |

> **为什么默认是 0.96 / 0.90**：e5-base 对"完全无关的中文短句"余弦相似度本来就有 **0.84–0.89**，
> 近重复才到 0.99+。早期用 0.90/0.75，导致每条新事件都被判成"相似"，全卡在待确认区。
> **换嵌入模型后请重新测量**：随便挑两句无关文本看基线分，再定阈值。

---

## 7. 待办（`[todo]`）

```toml
[todo]
max_pending_before_archive = 50   # 超过该值触发归档检查
archive_keep_count = 50           # 归档后保留的 pending 数
reminder_threshold = 30           # 超过该值生成"任务提醒"
archive_after_days = 30           # 只有超过该天数未更新的才算"陈旧"
```

> **语义要点**：数量超过 `max_pending_before_archive` 只是**触发检查**；
> 真正被归档的必须满足"陈旧"（`last_updated_at` 早于 `archive_after_days`）。
> **新鲜待办不会被自动隐藏**，这是刻意设计（回归测试 `test_todo_hot_cold_archive` 守着这条）。

---

## 8. 服务、存储与日志

```toml
[server]
host = "127.0.0.1"    # 默认只监听本机
port = 3000

[storage]
data_dir = "./data"
dictionary_dir = "dictionary"
relational_dir = "relational"
vector_dir = "vector_store"
relational_db_file = "relational.db"
vector_index_file = "vectors.hnsw"
question_pool_dir = "heartbeat/questions"

[logging]
level = "info"        # trace | debug | info | warn | error
dir = "./logs"
max_file_size_mb = 10

[event]
long_term_hot_limit = 200
short_term_hot_limit = 100

[agent]
primary_agent_id = "primary_agent_default"
internal_api_token = ""   # 用环境变量注入，不要提交
```

> `--host` 优先级高于 `[server]`；`--data-dir` 优先级高于 `[storage].data_dir`。
> 默认 `./data` 时保持历史布局（Sled 在仓库根的 `.sled_data`），
> 指定其它目录时所有数据统一收敛到该目录。

---

## 9. 常用组合示例

**A. 低内存机器（4 核 / 8 GB）**

```toml
[llm]
primary = "gguf"          # 或 "api"
[llm.gguf]
model_path = "./models/Ministral-3-3B-Reasoning-2512-Q8_0.gguf"
context_size = 2048
threads = 4
[heartbeat]
enabled = false           # 先关掉后台 LLM
```

**B. 已有 Ollama + 独显**

```toml
[llm]
primary = "ollama"
[llm.ollama]
model = "qwen2.5:14b"
```

**C. 完全离线、只跑夜间**

```toml
[heartbeat]
base_interval_secs = 600
max_interval_secs = 1800
[llm.gguf]
threads = 8
```
