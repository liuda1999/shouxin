# 架构设计

## 1. 定位

守心是**智能体的外挂记忆子系统**，不是面向人的笔记应用。

- 输入是「智能体 ↔ 用户」的对话（用户的发言 + 智能体的最终发言），智能体的内部推理不写入；
- 与智能体**并行运行**：写入不阻塞对话，处理慢慢来，读取的是已处理好的历史（最终一致）；
- **所有数据本地存储**，不联网、不依赖云服务。

设计价值观（顺序即优先级）：

1. **原文绝不丢失** —— 字典库是源文件，任何情况下不删除；
2. **写入不阻塞智能体** —— 写入路径只做落盘 + 入队；
3. **最终可查 > 实时一致**；
4. **自动化要可观测、可恢复、可解释**；
5. **本地优先**；
6. **有界资源 + 主动维护**。

---

## 2. 数据流

```
POST /api/memory/add
        │
        ├─▶ ① 原文落字典库（Sled，永不删除）
        ├─▶ ② 任务入队（Sled 持久化队列）
        └─▶ ③ 立即返回 conversation_id
                    │
                    ▼ 后台工作线程
        ④ 分块 + 提示词构造 ──▶ llama.cpp 抽取
                    │
                    ▼
        ⑤ 结构化解析（人物 / 事件 / 待办 / 知识）
           ├─ 模型输出脏字符 → 清洗后抽 JSON 片段
           ├─ 人名逐条校验（是否为真人名）
           └─ 失败/超时 → 正则降级（保证不丢数据）
                    │
                    ▼
        ⑥ 三库强事务写入（失败则补偿回滚）
           ├─ 关系库 SQLite：结构化索引（可查询、可排序、可归档）
           ├─ 向量库 HNSW：语义检索
           └─ 字典库 Sled：原文与实体档案
```

**可恢复性**：队列持久化在 Sled 中，进程崩溃后重启可续跑；
每条对话都有 `extract_status`，失败可用 `/api/memory/replay/*` 重放。

---

## 3. 三库分工

| 库 | 实现 | 存什么 | 特性 |
|----|------|--------|------|
| 关系库 | SQLite（WAL、sqlx） | 事件、联系人、待办、文件元数据等结构化记录 | 支持条件查询、排序、冷热分层；事务强一致 |
| 向量库 | HNSW（`hnsw_rs`）/ 内置暴力检索 | 事件、联系人、知识、事件的语义向量 | 近似最近邻；小数据量时可用精确检索 |
| 字典库 | Sled | 原文对话、实体档案、任务队列 | KV，**永不删除**，是所有记忆的源文件 |

**一致性**：写入走三库强事务 + 补偿机制。任一库写失败会回滚已成功的部分，
并把任务标记为可重放，避免"关系库有、向量库没有"的半截状态。

---

## 4. 事件生命周期

```
抽取出事件
   │
   ├─ 与已有事件相似度 < 0.75 ──▶ 直接新建
   ├─ 0.75 ~ 0.90 ─────────────▶ 进 pending_merges，等人工/心跳确认
   └─ > 0.90 ──────────────────▶ 自动合并（更新活跃度与时间）
```

- **待确认队列**必须有出口：`GET /api/events/pending_merges` 查询，
  `POST /api/events/pending_merges/:id/resolve` 确认（merge / create / dismiss）。
  没有这个出口，0.75–0.90 区间的事件永远进不了索引 —— 这是早期版本的一个真实缺口。
- **热冷分层**：热库有容量上限，超出后按活跃度归档到冷库；
  归档数据**默认参与检索，但降一级优先级**（不是排除）。

---

## 5. 待办

- 识别待办与截止时间；
- **截止时间优先本地解析**：`utils/time_parser` 从原文挑"与待办标题重合度最高的句子"，
  只认能唯一确定某一天的表达（今天/明天/后天/周X/下周X/X月X日/X号/月底/月初），
  **明确排除**"下季度""下半年"这类时间范围；模型给出的日期仅在本地解析失败时兜底。
  原因：小模型给的 ISO 日期经常漂移，甚至给出过去的时间。
- **标题模糊匹配**用于"按对话更新待办"：采用**子序列匹配**而非 `LIKE '%关键词%'`，
  使"写报告"能匹配到"写项目报告"。

---

## 6. 联系人

- 从对话中识别人名（LLM 判定 + 正则兜底 + 昵称规则）；
  **新增硬性校验：姓名或其别名必须逐字出现在对话原文中**，否则丢弃
  （防止小模型照抄提示词 few-shot 示例里的人名，凭空造出"李总的女儿""张总的秘书"）。
- 维护联系人档案：最近互动时间、关联事件、称呼变体；
- **虚拟联系人**（`/api/virtual_entities`）：为智能体扮演的角色维护人设资料；
- 联系人同样走热冷分层。

### 6.1 画像（人设）与关系是怎么自动生成的

由**心跳**驱动（`HeartbeatAnalyzer::analyze_contact_relations` → `generate_contact_profile`）：

1. 选人：`background` 为空、7 天内有互动、非自身/非归档/非虚拟的联系人，每轮最多 3 个；
2. 取证：该联系人的事件摘要 + **最近含其姓名/别名的 3 段对话原文**
   （`get_recent_conversations_for_owner`，不区分抽取状态 —— 注意
   `get_conversations_since_v2` 只返回 pending/failed，不能拿来做历史召回）；
3. 生成：LLM 输出联系人画像 JSON（`personal_info` / `social_relations` / `major_events` /
   `objective_evaluation` / `potential_relations`）。提示词要求 **key 保持英文、value 用中文**，
   否则模型会把 key 也翻译掉，导致解析全空；
4. 落库：
   - 整份画像 → `contact_index.background`（JSON）
   - `social_relations[].relation_type` → `contact_index.relationship_tags`
   - `social_relations[].related_contact_name` → 在同一 owner 内解析成 ID →
     `contact_index.related_contact_ids`（跨 owner 不解析，避免误关联）
5. 频率：单联系人每天 ≤ 2 次、间隔 ≥ 8 小时。

检索侧（`POST /api/memory/search/contacts`）返回的 `aliases` 是
`contact_index.aliases` 与 `contact_alias_index` 的**并集**，
`related_contacts` 已把 ID 解析为姓名，`is_virtual` 用于区分真实联系人与虚拟角色。

### 6.2 联系人全息详情（三库联查）

`GET /api/contacts/:id/detail` 是唯一一个同时读三库的联系人接口：

| 数据 | 来源 | 边界 |
|------|------|------|
| 人设 / 别名 / 关系标签 / `is_virtual` | SQLite `contact_index` | 单条主键查询 |
| 关联联系人（ID → 姓名 + 标签） | SQLite `contact_index` 批量 | 由 `related_contact_ids` 决定 |
| 关联事件 | SQLite `event_index.contact_ids` 反查 | `event_limit`，上限 100 |
| 相关记忆原文 | Sled 对话时间索引倒序扫描 | 扫描上界 500 条，返回 `memory_limit`，上限 50 |

为什么要单独做：`search_contacts` 只返回联系人自身字段，调用方拿不到
"他参与过哪些事件""哪段对话提到过他"。智能体要做人物关系推理
（谁是谁的上级、谁和谁一个项目组），必须一次拿到 **关系 + 事件 + 原文证据**。

> 记忆匹配用的是"姓名或别名逐字命中"，不是向量相似度。
> 对"这个人出现在哪些对话里"这类**精确溯源**需求，逐字命中比语义召回更可靠，
> 也不会因为 embedding 漂移把无关对话算进来。

> 早期版本这条链路有 6 处断点（读取硬编码空值、模板/提示词不匹配、关系列从不写等），
> 表现为"接口永远返回空"，详见 [../CHANGELOG.md](../CHANGELOG.md) 的 C1–C7。

---

## 7. 心跳

心跳是**清洁工 + 资料员**，不是简单的定时任务：

- **清洁工**：巡检待办（到期提醒）、处理积压任务、清理无效中间态；
- **资料员**：更新联系人档案、生成待确认问题（`pending_questions`）等用户输入；
- **自适应频率**：根据空闲时长、积压量在 `min_interval_secs ~ max_interval_secs` 之间调整；
- **资源保护**：超过内存阈值会跳过本轮（`max_consecutive_skips` 后强制降频）；
- `POST /api/heartbeat/trigger {"maintenance_only": true}` 只做维护、不调 LLM，用于排查。

---

## 8. 检索

多路召回 + 融合排序：

| 通道 | 用途 |
|------|------|
| 语义（向量） | 模糊意图、"意思相近"的检索 |
| 事件（结构化 + 向量） | 按时间/参与人/主题找事件 |
| 联系人 | 按名字/昵称/关系找人和其资料 |
| 历史对话 | 原文级回溯 |

`POST /api/search/smart` 会把多路结果融合并按相关度 + 时间衰减 + 冷热等级重排。

---

## 8.5 资源占用原则（后台服务）

守心与其它服务共存，资源策略是**硬约束**，不是建议：

| 资源 | 策略 | 实现位置 |
|------|------|----------|
| **显存** | **默认完全不用**。`use_gpu` 在配置缺省与代码缺省下均为 `false`，`n_gpu_layers` 恒为 0；只有显式 `use_gpu = true` 且以 `--features gguf-cuda` 编译才会卸载到 GPU | `config/settings.rs::default_gguf_use_gpu`、`llm/gguf.rs` |
| **CPU（GPU 模式）** | 线程数硬性上限 = `核心数 × gpu_max_cpu_percent%`（默认 60%，向下取整，最少 1）。32 核机器最多 19 线程 | `llm/gguf.rs::resolve_threads` / `cap_threads_for_gpu` |
| **CPU（纯 CPU 模式）** | 用配置的 `threads`，且不超过核心数 | 同上 |
| 内存 | 模型 mmap 驻留，受 `[heartbeat].memory_threshold_mb` 保护，超阈值跳过心跳 | `service/heartbeat*` |

判定逻辑（单测覆盖）：

```rust
resolve_threads(configured, cores, gpu_enabled, percent)
  = gpu_enabled ? min(max(configured,1), max(1, cores*percent/100))
                : min(max(configured,1), max(cores,1))
```

日志证据（启动时一行即可确认）：

```
[GGUF] 初始化: GPU=false, n_gpu_layers=0, context=4096, threads=16（核心 32，GPU 上限 60%）
[GGUF] GPU 模式限制 CPU 占用：threads 32 -> 19（核心 32，上限 60%）   # GPU 模式且被压制时
```

---

## 8.6 磁盘 I/O 策略（硬盘寿命保护）

守心是常驻后台服务，**不能靠轮询把磁盘读穿**。当前策略：

| 操作 | 频率 | 说明 |
|------|------|------|
| 心跳调度 | 60–120s（自适应） | 无任务时一轮约 2–4 ms，不触发 LLM |
| 孤儿向量清理 | **≤ 每 6 小时一次** | 需全量读向量库，因此从"每轮心跳"降为限流执行 |
| 对话时间索引查询 | 按需 | 用 `range(since..)` 直接定位，不做全表扫描 |
| 画像取证 | 每次画像 1 次 | 最多倒扫 500 条索引，避免退化为整库遍历 |
| 向量写入 | 按需 | **不逐条 fsync**，由 sled 周期落盘 + 关闭时 flush |
| HNSW 落盘 | 每 5 秒批量 | `flush_every_ms(5000)` |

实测空转（Ministral-3B、无请求）约 **122 KB/min 读、10.7 KB/min 写**，
折算约 0.5 GB/月写入。

> 修改涉及"全量遍历/扫描"的逻辑时，请先估算代价并**加限流或索引定位**；
> 不要把 O(全库) 的操作放进心跳周期里。

---

## 9. 降级与可观测

系统有多层降级，**每一层都必须留下明确日志**，否则会静默劣化：

| 层 | 降级行为 | 日志特征 |
|----|----------|----------|
| 嵌入 | ONNX 模型不可用 → 离线哈希嵌入 | `Embedder initialized (backend: hash, ...)` |
| 抽取 | LLM 失败/超时 → 正则提取 | `falling back to regex extraction` |
| 模型输出 | 含乱码/截断 → 清洗 + 抽 JSON 片段 | `Detected garbage characters` |
| 事件写 | 相似度落中间区间 → 待确认 | `pending_event_merge` |

> 经验：降级"成功返回"比直接报错更危险。接口显示 `completed` 并不代表走的是 LLM 路径，
> 排查质量问题时**先看日志里的降级痕迹**（见 [TROUBLESHOOTING.md](TROUBLESHOOTING.md) 2.3）。

---

## 10. 关键模块

```
src/
├── api/routes.rs             HTTP 路由与处理器
├── config/settings.rs        配置结构与加载（含 SHOUXIN_CONFIG）
├── core/                     领域模型
├── embedder/                 嵌入器（fastembed / hash）+ 工厂
├── llm/
│   ├── gguf.rs               内置 llama.cpp 客户端（CPU 推理）
│   ├── ollama.rs / api.rs    其它后端
│   ├── factory.rs            按配置装配客户端（含心跳专用客户端）
│   └── manager.rs            主备切换与健康检查
├── memory_extractor/         提示词、解析、正则降级
├── service/
│   ├── background_queue.rs   异步抽取队列（含待办 deadline 处理）
│   ├── heartbeat*.rs         心跳调度与执行
│   └── transaction_writer    三库事务写入与补偿
├── stores/relational|vector|dictionary
└── utils/time_parser.rs      中文相对时间解析
```
