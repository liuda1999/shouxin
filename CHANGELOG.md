# 变更记录

## [0.6.4] 2026-10-30 — 待办完成幂等：重复标记不再改写完成时间

真实使用模拟（创建 → 完成 → 再完成 → 删除）时发现：`PUT /api/todos/:id/complete`
重复调用会把 `completed_at` 覆盖成最新时间（实测 `06:30:09` → `06:30:13`）。
而心跳与对话抽取会**反复**判定同一待办"已完成"，等于完成时间会随每次巡检向后漂移，
"这件活到底什么时候干完的"这一条审计信息就失真了。

### 修复：`completed_at` 只认首次

`src/stores/relational/todo_list.rs` 的 `complete_todo` 改为：

```sql
SET status = 'completed',
    completed_at = COALESCE(completed_at, ?),
    last_updated_at = ?
```

- 采用**只冻结 `completed_at`、`last_updated_at` 照常刷新**的方案：重复调用仍视为"又碰了一次"，
  但不改变完成时刻；
- 不改 bind 顺序、不加控制流，单文件单语句，**无 schema 变更、无迁移、无 API 契约变化**。

**为什么不用 `WHERE status != 'completed'`**：现有代码用 `rows_affected() == 0` 判断"Todo not found"，
加上该条件后重复完成会命中 0 行 → 被误判成 404/500。`COALESCE` 让匹配行数仍为 1，错误语义完全不变。

### 影响面（改动前已逐项核查）

| 依赖方 | 结论 |
|---|---|
| 待办冷热归档 | 只处理 `status='pending'` 且用 `last_updated_at`，**不读** `completed_at` |
| 事件过期清理（`maintenance.rs`） | 读的是 **event_index 自己的** `completed_at`，与待办无关 |
| 统计 / 画像 | 全仓库无按待办 `completed_at` 的聚合；画像不读待办清单 |
| 列表 / 详情 | 仅展示，`ORDER BY deadline`，不做时间排序 |
| 现有测试 | 不检查时间值，不会因本次改动失败 |

### 测试

- 新增 `tests/test_todo_complete_idempotent.rs`（4 项）：重复完成时间不变、`last_updated_at` 仍刷新、
  不存在的 id 仍报错、其它字段不被篡改；
- `cargo test --features gguf --lib`：**101 passed / 0 failed**；
- release 二进制端到端（真实 HTTP）：**15/15 通过**，三次调用 `completed_at` 恒为 `06:42:36.233346500Z`，
  `last_updated_at` 正常前移；`api_smoke_test.sh`：**PASS=37 / FAIL=0 / SKIP=0**。

### 文档与版本

- `docs/API.md` 待办章节：补充幂等语义，并说明**没有字段级更新接口**（`PUT/PATCH /api/todos/:id` → 405，
  改截止时间走对话抽取的 `update_todo_deadline`，且只对 pending 生效）；
- `docs/ARCHITECTURE.md` §5：补"完成是幂等的"设计理由；
- 过时数字修正：README「当前版本 0.6.0 → 0.6.4」、单元测试 89 → **101** 项、
  集成测试 10 → **11** 个文件、冒烟 36 → **37** 项；
- `docs/MODEL_SETUP.md`：mmproj 说明里的"当前版本（v0.5.0）"→（v0.6.x）；
- `Cargo.toml` / `Cargo.lock`：0.6.3 → 0.6.4。

### 未做（按用户意见保留）

- 字段级更新接口（`PATCH /api/todos/:id`）**不实现**，已在 API 文档中说明替代路径；
- 存量数据里已被污染的 `completed_at` 不自动回填（无法区分"真实完成时间"与"最后一次重复标记"）。

---

## [0.6.3] 2026-10-30 — 配置标注：云端 API 示例修正与两条已知限制

专项检查"关键配置页能不能找到、能不能把内置模型换成云端 API"时，确认配置文档体系本身是完整的
（`config.toml` + `docs/CONFIGURATION.md` + README 直达链接 + `scripts/check_config.py`），
但云端这条路有三处会误导人的地方。本次只做**最小改动**：修示例、标限制，**不实现**新功能。

### 修正：endpoint 必须写全路径

`src/llm/api.rs` 把 `endpoint` 直接当 POST 目标（`.post(&self.endpoint)`），不做 `/chat/completions` 补全。
而 `config.toml`、`config.toml.example`、`docs/CONFIGURATION.md` 三处示例原来都写成 `https://api.openai.com/v1`，
照抄必然 404/405。

- 三处示例统一改为 `https://api.openai.com/v1/chat/completions`，并注明"不会自动补全"；
- `ApiClient::new()` 加一行注释点明该约定（无行为变更）。

### 标注：心跳侧暂不支持 API 密钥

`HeartbeatLlmSettings` 没有 `api_key` / `headers` 字段，工厂心跳分支只能传空密钥
（`src/llm/factory.rs` 原先就硬编码空串，本次未改行为，只补 `TODO(cloud-api)` 注释）。
影响：`[llm.heartbeat].backend = "api"` 只能对接无需鉴权的本地/自建网关。

### 标注：换主链路不会停掉本地模型

`primary = "api"` 只影响主链路，`[llm.heartbeat].backend` 默认仍是 `gguf`，启动照旧加载 4.7 GB 本地模型。
受上一条限制影响，目前无法仅靠改配置彻底切到云端。

### 其它

- `docs/CONFIGURATION.md` §2.3 增加"状态：已实现但未做端到端验证"提示与两条限制清单；
- README「配置」节加一句指向该提示；版本徽章 0.6.2 → 0.6.3；
- `Cargo.toml` 版本 0.6.0 → 0.6.3（此前未随 tag 同步，`--version` 输出与 Release 对齐）；
- 仓库内仍无任何配置档把 `primary` 设为 `api`，也无可运行的端到端验证 —— 属已知缺口，未在本次处理。

---

## 0.6.3 之前的一轮收尾 — 启动自检：清掉一条误报，补齐"什么日志算正常"

收尾重启验证时，启动日志里只剩一条 WARN：`备模型客户端创建失败: 未知的 LLM 来源: none`。
它其实是误报——`secondary = "none"` 是**显式关闭备模型**，配置层却把它当成一个后端名去构客户端，
失败后打 WARN，同时 `auto_switch` 被误置为 true（没有备模型却以为有）。

- 配置层：`secondary` 为空或 `"none"` 时归一为 `None`；
- 工厂层：同样过滤 `none`/空串，`auto_switch` 改为按"是否真的建成了备客户端"判断；
- 结果：干净启动只剩 `WARN Admin API token is empty`（未配 `INTERNAL_API_TOKEN` 时的预期提示）。

顺带把心跳日志的措辞改准：`Manually triggering full heartbeat` → `Triggering full heartbeat`。
调度器的定时首轮与 `POST /api/heartbeat/trigger` 走同一个函数，原来一律写"手动触发"，
排查时容易误判成"有人在调接口"。

同时给 `docs/TROUBLESHOOTING.md` 补了「1.1.1 启动自检清单」：
正常启动的日志序列、哪些 WARN 可以忽略、哪些 ERROR 必须处理、
以及用 `/api/heartbeat/status` 判断心跳是否待命。

### 本次启动实测

- 启动 → `Server listening` 约 4 s（模型已缓存），全程 **0 ERROR**、**1 条预期 WARN**
- 心跳首次 30 s 触发，首轮 27.8 s 完成（3 个联系人画像更新），之后按 120 s 自适应周期继续
- `/api/heartbeat/status`：`is_running=true`、`consecutive_skips=0`、可用内存 38 GB
- 功能冒烟：写入 → 抽取 completed → 联系人「孙倩」、事件「周会」、待办「整理上周的销售数据…」截止 11-04；
  字典库按天分页读到当天 1 条，原文回读一致
- 内存：RSS ≈ 11.6 GB（主客户端 + 心跳客户端各持一份 4.7 GB 模型句柄）

## [0.6.2] 2026-10-30 — 复查排队结果：Gemma 走错模板（抽取/画像全线失真的根因）

排队任务跑完后复查，发现产物质量整体失真：12 段对话只抽出 9 个碎片联系人、
26 次画像尝试 **0 条落库**、事件标题是"er: 我让龙飞整理"这类碎片。
顺着日志挖下去，根因不是模型能力，而是三处代码问题。

### 根因 1：所有模型都被当成 Mistral 喂提示词

`build_chat_prompt` 在按家族判断**之前**就无条件 `return "[INST] ..."`，
后面的 Gemma / Qwen 分支永远执行不到。`config.toml` 用的正是 Gemma，
于是每次推理（22/22、41/41、25/25 次）都拿到它看不懂的 `[INST]` 格式，
模型只回 `{"people":[],"events":[],"todos":[]}` 甚至空串，抽取全靠正则兜底。

修复：先判家族（`general.architecture` / 模板串 / `general.name`），
Gemma → `<start_of_turn>`，Qwen → `<|im_start|>`，其余才走 `[INST]`；
Gemma/Qwen 不再尝试必然失败的 Jinja 渲染，WARN 噪声一并消失。

**效果（同一台机器、同一个模型）**

| 指标 | 修复前 | 修复后 |
|------|--------|--------|
| 抽取输出 | 0–74 字符（空 JSON） | 427–541 字符完整 JSON，9–20s |
| 画像落库 | 0/26 | 6/7 联系人（含关系标签），9–11s/条 |
| 联系人质量 | 15 个里 8 个是句子碎片 | 7 个全是真人名 |

### 根因 2：画像超时 90s，且整轮预算 120s

CPU 上 4B 模型生成 500 token 的画像 JSON 需要数分钟，`Duration::from_secs(90)`
硬编码让每次尝试都在 90s 被掐断；外层整轮又是 120s，即使单次放宽也会被整轮截断。

修复：新增 `[heartbeat].profile_timeout_secs`（默认 240），
整轮超时取 `max(llm_timeout_secs, profile_timeout_secs + 60)`。

### 根因 3：卡在 processing 的对话永远不会被重放

队列对"把进程搞崩 3 次"的任务会直接丢弃（`retry_count > 3`），但字典库状态停在
`processing`——既不在 pending，也不在 failed 列表里，永远查不到、也重放不了
（本轮复查就有 1 条）。修复：启动时 `reset_stale_processing()` 把它们标成
`failed` 并写明原因，随后队列恢复会正常重跑。

### 顺带修正

- **截止时间取错**：一句话里同时有"周六要谈续约……下周一前给我报价单"时，
  原实现取第一个时间表达，把截止日算成周六。改为优先认 `X 前 / X 之前 / X 以前 / 截止 X`。
- 事件标题兜底回归测试补齐（角色前缀、句子边界）。

### 验证

- 单测 **93/93**
- 干净目录跑通 12 段对话：**12/12 completed**，联系人 7（罗军/老周/王强/陈静/李娜/张伟/汤敏，全部真人）、
  事件 9、待办 6（截止日 11-04 / 11-06 正确）、画像 6/7
- 定向用例："周六…下周一前给我报价单" → 截止日 2026-11-02（下周一）✅

## [0.6.1] 2026-10-30 — 字典库：时间戳 + 按天分页 + 跨零点兜底

这一版只解决一件事：**让"字典库按天分页"这个原设计真正落地，并且经得起跨零点**。

### 原设计复原

原设计里字典库存的是"每次写入的原始对话"，要求：
① 每条都带时间戳；② 数据按天分页；③ 遇到凌晨（跨零点）要有兜底，不能把一次会话切成两页。

核查结果：① 有 `timestamp` 但只能由服务端取 `Utc::now()`，导入历史时无法指定真实时间；
② 键是 `{UTC日期}:{id}`，**按自然日而不是会话日**分页，且没有任何读接口；
③ **跨零点兜底完全缺失**——23:50 和 00:10 的对话会被分到两页，且 UTC+8 的凌晨 0–8 点还会整体落到前一天。

### 修复

| 项 | 实现 |
|----|------|
| 时间戳可指定 | `POST /api/memory/add` 新增 `timestamp`（别名 `occurred_at`），支持 RFC3339 / `YYYY-MM-DD HH:MM[:SS]` / `YYYY-MM-DD`；非法、未来 1 天以上、早于 2000 年一律 400 |
| 入库时间分离 | 记录新增 `ingested_at`，与对话发生时间分开，导入历史时可审计 |
| **会话日**（跨零点兜底） | 新增 `session_day`：以本地时间 `[storage].day_cutoff_hour`（默认 **04:00**）为界，00:00–03:59 归**前一天**；写入时固化，查询时再用时间戳重算比对，边界不串页 |
| 本地时区分页 | 分页按本地日期而非 UTC 日期（可用 `[storage].utc_offset_minutes=480` 钉死），修掉"早上 8 点前的对话算昨天" |
| 日索引 | 新增 `conversation_day_stats`（总数 / 按 owner 计数 / 首末时间），与原文同一 sled 事务写入；旧库首启自动重建（`meta.day_index_version`），不重写旧键 |
| 读接口（此前只写不读） | `GET /api/dictionary/days`、`/day/:date`、`/conversation/:id`、`/stats`、`/paging` |
| 联系人事件档案可读 | `GET /api/contacts/:id/event_archive`（这份档案一直在写，但没有任何读接口） |
| 写入原子性 | 原文 / 时间索引 / ID 索引 / 日索引 / 待提取任务统一进同一事务；同一 id 重写会自动从旧会话日摘除，计数不重复 |
| 范围查询修正 | `get_conversation_by_time_range` 改为走时间索引区间（原来按自然日前缀拼，跨 cutoff 必漏） |
| **兜底抽取不再造人** | 正则兜底里"任意 2–4 个汉字即人名"的分支被删除。它曾把"复盘""大家一致""主要是需""五前交""评估表周""龙飞转正"等句子碎片写成联系人（一次跑通 15 个联系人里 8 个是假的）。宁可这段对话不产生联系人，也不留假档案 |
| 兜底事件标题修正 | 标题原来是"关键词前 8 字"，于是出现 `er: 我让龙飞整理`（半个 `user:` 前缀）、`飞转正，让我准备评估`（从半个词开始）。改为：先剥离角色前缀，再向左/右取到句子边界（右向上限 24 字），两条兜底路径共用同一窗口函数 |
| 事件检索接受 `owner_id` | `POST /api/memory/search/events` 的 `user_id` 增加 `owner_id` 别名。此前调用方写 `owner_id` 会被静默忽略，接口返回空列表而 `success: true`，最难排查 |

### 清理

- 删除 `stores/dictionary/conversation.rs`（`ConversationStore`）：与 `DictionaryStore` 重复且从未被实例化；
- 删除 `stores/relational/contact_background.rs`（`ContactBackgroundSummarizer`）：已被心跳的画像生成取代，同样从未接线。

### 验证

- 单测 **90/90** 通过，其中新增 8 项分页专项（cutoff 边界、跨零点、翻页不重不漏、重写不重复计数、owner 过滤、统计口径）；
- 实机 `scripts/dictionary_paging_test.py` **28/28** 通过（含 23:40→00:30→03:59 同页、04:00 整点换页、非法/未来时间戳拒绝、跨 owner 404）；
- 端到端跑通见下节。

## [未发布] 2026-10-30 — 验收审计 v2：225 条全新语料 + 时间线与画像链路修复

### 本轮测出来的问题（按严重度）

| 级别 | 问题 | 修复 |
|------|------|------|
| **P1** | 事件时间只对 `time_info` 做 RFC3339 解析，遇到"上周三"直接失败并回落到**写入时间**——所有历史事件都变成"今天"，"查某人近一个月的事"完全失真 | 新增 `resolve_event_time`：`time_info` → `time` → 事件描述 → **对话原文**，逐级降级 |
| **P1** | 时间解析器只支持"今天/明天/后天/N天后"，**不支持过去时间** | 补齐：昨天/前天/大前天 / N天前 / N周前 / N个月前（含中文数字）/ 上周X / 上上周X / 上个月（初/中旬/底）/ 去年X月；11 项单测 |
| P2 | 模型把时间词从描述里删掉时无兜底 | 加"对话原文"兜底解析 |
| P2 | 人名过滤漏句子碎片（"别忘""前采购服""本周五之""接口文档"），它们抢占画像候选位却永远生成不出画像 | 过滤补三类：时间短语、动词前缀、事物词；单测同步 |
| **P1（未修）** | 歧义检测把不同人的别名混成一人，并重复创建"已存在"的待办 20+ 次，吃光心跳预算导致画像不推进 | 已记录，待改（见 docs/ACCEPTANCE_V2.md 4.5） |
| P2（未修） | 画像空响应时按"每日 30 次"重试，一个坏联系人反复占名额 | 建议改为连续失败 2 次即跳过（4.6） |
| P2（未修） | 事件合并只更新 `contact_ids` 不更新时间，重放无法修正历史事件时间 | 建议合并时若旧时间为空/等于创建时间则采纳新时间（4.7） |

### 验收结果（225 条全新语料，不复用历史用例）

- 抽取 **224/225** 完成，**0 失败**，空输入按预期被拒；**0 崩溃**
- 联系人 **164** 条，人名召回 **84.8%**（39/46），**凭空捏造 0**
  - 未命中 7 个中 4 个是虚拟角色（本就走独立表），真实漏抽 3 个
- 别名识别 **93.3%**（42/45），两个口径（抽进 aliases / 用别名搜到本人）一致
- 画像 **39/164 = 23.8%**，合法 JSON + 5 字段 **100%**（覆盖率受 CPU 算力与心跳调度限制）
- 关系 **10/37 = 27%**，可评估口径（至少一方有画像）**10/21 = 47.6%**；机制通，准确率受模型限制
- 时间线：事件带时间 **100%**，`since=30d` 过滤 **8/8 正确**
- API 覆盖 **28/28 = 100%**

详见 **[docs/ACCEPTANCE_V2.md](docs/ACCEPTANCE_V2.md)**。

### 新增脚本

- `scripts/corpus_v2.py`：225 条全新语料（别名/关系/时间线/同名/边界/虚拟角色/长文）
- `scripts/acceptance_v2.py`：提交 / 轮询 / 六项审计（含 API 覆盖 28 项）

## [0.6.0] 2026-10-30 — 联系人体系重构：独立编号 + 虚拟角色拆表 + 分词器加固

这一版主要做一件事：**把"人"这件事做对**。联系人不再是"一个名字"，
而是 编号 / 姓名 / 别名 / 消歧标签 / 关系标签 / 身份线索 / 画像 / 事件 的组合。

### 新增能力

- **联系人全息详情** `GET /api/contacts/:id/detail`：一次返回 人设 + 关联联系人（含各自标签）
  + 关联事件 + 相关记忆原文（按命中词截取，不再是死取前 300 字）。
- **虚拟角色独立建表**（`virtual_character` / `virtual_character_alias` / `event_character_link`）：
  历史/神话/动漫/游戏角色与真人隔离，同名不同作品可共存（雅典娜 ×2）。
- **同名联系人支持**：`contact_index` 主键仍是 UUID，姓名不再唯一；新增 `disambiguation` 字段，
  两个"刘洋"各有一个编号，检索返回多条、靠标签区分。
- **多字段同名匹配**：姓名 + 消歧标签 + 别名 + 身份线索（`role`）加权投票，
  跨对话也能认出"这是上周那个前端刘洋"。
- **冷热两级检索**：先查热表，不够再回退冷表 —— 人被归档后依然搜得到。
- **画像限流可配置**：`[heartbeat] profile_max_updates_per_day` / `profile_update_interval_hours`，
  批量导入场景可调大；没画像的一律补，今天没被提到的不更新。

### 修复的缺陷

| 级别 | 问题 |
|------|------|
| **P0** | llama.cpp 分词正则触发 C++ 异常跨 FFI，**整个进程 abort**（压测复现 4 次）。加固：提示词字符白名单、软换行、128 字符分段分词、32 MB 线程栈、病态输入跳过 LLM |
| P0 | 崩溃任务无限重试（abort 走不到失败计数）→ 恢复时累加 `retry_count`，超 3 次放弃 |
| P1 | 归档联系人导致唯一索引冲突，**整条对话的 contacts/events/todos 全部回滚** |
| P1 | 心跳重复抽取全部历史对话，与后台队列抢模型（一启动就重跑 144 条） |
| P1 | 歧义检测提示词 47k 字符，远超 4096 上下文 |
| P2 | 默认截止日按 UTC 计算，UTC+8 凌晨会早一天 |
| P2 | 关系短语/组织名被当人名（"小明的姐姐""客户的工程师""外包团队"） |
| P2 | 画像补齐被 7 天窗口硬卡，老联系人永远补不上 |

### 破坏性变更

1. **虚拟角色不再存于 `contact_index`**：角色详情改走 `/api/virtual_entities/:id`，
   不再复用 `/api/contacts/:id/detail`；`POST /api/memory/search/contacts` 默认
   不含虚拟角色（需要时传 `include_virtual: true`）。
2. **姓名不再唯一**：`UNIQUE(name, owner_id, owner_type)` 已删除（老库启动时自动 DROP）。
   客户端请用 `contact_id` 引用联系人；同名的靠 `disambiguation` 展示区分。
3. 新增列 `contact_index.disambiguation`（自动 `ALTER TABLE`，老库无需手工迁移）。

### 升级说明

- 直接替换二进制即可：启动时会自动建虚拟角色表、加 `disambiguation` 列、删除名称唯一索引。
- 若从 0.5.0 升级且库里有 `is_virtual = 1` 的联系人行，它们**不会**自动搬到新表
  （本机测试库当时是直接清空重建的）。需要保留的话请先导出，或按
  `virtual_character` 结构手工导入。

---

## 0.6.0 变更明细（按提交时间倒序）

### 2026-10-30 — 同名匹配升级为多字段投票（联系人不是"一个名字"）

### 上一轮把"限制"说错了

上一轮我说"跨对话同名无法自动关联"—— 这不成立。联系人不是单一字段，
而是 姓名 / 别名 / 消歧标签 / 关系标签 / 备注（身份线索）/ 画像 / 关联事件 的组合，
检索返回里也有全局唯一的 `contact_id`。同名本来就能区分，问题在于**匹配时只用了姓名**。

### 改法

1. **抽取层补 `role`（身份线索）**：`ExtractedPerson.role` 字段一直存在但提示词从不要求，
   现在要求模型输出"部门/职位/关系"（`前端工程师`、`财务部`、`我的大学室友`），无依据时留空。
2. **写入时落库**：`role` 写进 `notes`（`身份：前端工程师`），成为下次匹配的依据。
3. **匹配改为多字段打分** `find_contact_for_person_with_hints(name, disambiguation, aliases, role)`：

| 信号 | 分值 |
|------|------|
| 消歧标签完全一致 | +100 |
| 消歧标签为空（还没被区分出来） | +5 |
| 消歧标签不同且非空（明确是别人） | −100 |
| `role` 与候选的 关系标签/备注/画像 互相包含 | +40 |
| `role` 与候选身份**同类但取值不同**（前端工程师 vs 后端工程师） | −40 |
| 本次别名与候选姓名/别名有交集 | +30 |
| 其余按最近交互顺序 | +10…+1 |

   判定：最高分 ≤ 0 → 新建；本次带标签但最佳候选已有**别的**标签 → 新建；否则复用。

4. **关系解析不再盲取第一条**：`find_contact_id_by_name_same_owner` 原来是 `LIMIT 1` 无排序，
   同名时会把"张三的上级"挂到另一个张三身上。现在取候选后按
   共同关系标签（×3）/ 有画像（+2）/ 最近交互 / 归档降权 打分选最优。

### 验证

- 单元测试 **89 项全过**，新增 3 个同名回归用例：
  `test_same_name_cross_conversation_matches_by_role`、
  `test_same_name_conflicting_roles_create_two_contacts`、
  `test_same_name_without_hints_falls_back_to_recent`
- 真实抽取链路（同一用户连续 4 条，逐步加信息）：

```
step1「刘洋是我同事，前端工程师」      → 1 条：刘洋[空]  notes=身份：前端工程师
step2「另一个刘洋是后端工程师」        → 2 条：+刘洋[后端]（身份冲突 → 分开）
step3「刘洋说接口调通了」（无线索）    → 2 条（归到最近互动，不新建）
step4「前端工程师刘洋在评审页面」      → 2 条：刘洋[空] 补成 [前端]（认出是同一人）
```

- `scripts/api_acceptance.py`：**PASS=23 FAIL=0**
- 全接口冒烟：**PASS=36 FAIL=0 SKIP=1**，0 ERROR

### 还剩的边界

- 本次提取既没给 `disambiguation`、也没给 `role` 时，同名多条只能按"最近互动"归并
  （信息不足时的保守选择，不会乱建号）。
- 事件共现（谁和谁一起出现在同一事件里）暂未参与打分，留作后续增强。

### 2026-10-30 — 联系人姓名不再唯一：独立编号 + 同名消歧标签

### 核实结论

"每个联系人一个独立编号"这半部分原本就是对的：`contact_index.id` 是 UUID 主键，
详情、事件、记忆全按 id 走。但"不怕重名"没有实现，被三处抵消：

| 位置 | 原来 | 后果 |
|------|------|------|
| `store.rs` 建表 | `CREATE UNIQUE INDEX idx_unique_contact_name_owner ON contact_index(name, owner_id, owner_type)` | 两个"刘洋"根本插不进去；归档行仍占着名字，新写入拿新 UUID 就撞索引 → **整条对话的 contacts/events/todos 全部回滚** |
| `writer.rs` 去重 | `find_contact_by_any_name` 按姓名/别名匹配，命中即复用旧 id | 同名即同一人，两个刘洋被静默合并 |
| 抽取层 | 只输出姓名，没有消歧信息 | 即使库允许重名，也没有依据判断"这是不是另一个人" |

### 改法

1. **删除唯一索引**，改为普通索引 `idx_contact_name_owner`；老库启动时 `DROP INDEX` 后重建。
2. `contact_index` / `ContactTotal` / `ContactDetail` / `ContactPatch` 增加 `disambiguation` 字段。
3. 抽取提示词新增规则：**仅当同一段对话里出现两个同名的人**时填消歧标签，
   并且"区分词不能并进姓名"（`前端刘洋` → `name=刘洋` + `disambiguation=前端`）。
4. 写入去重改为 `find_contact_for_person(name, disambiguation)`：

| 新提取 | 库里已有 | 结果 |
|--------|---------|------|
| 刘洋+前端 | 刘洋[前端] | 复用 |
| 刘洋+前端 | 刘洋[空] | 复用并补标签 |
| 刘洋+前端 | 刘洋[后端] | **新建独立编号** |
| 刘洋（无标签） | 都是带标签的刘洋 | 归到最近互动的那个，不新建 |

5. **姓名归一化兜底**：模型偶尔把区分词并进姓名（实测第二次提及输出 `name="前端刘洋"`），
   写入时若去掉前缀后能与库中已有联系人**精确同名**，就拆成 `刘洋` + `前端`，
   否则保持原名（不凭猜测改人名）。

### 验证

- 单元测试新增 `test_same_name_different_disambiguation_creates_two_contacts`：**86 项全过**
- `scripts/api_acceptance.py` 新增 4 项同名检查：**PASS=23 FAIL=0**
  （两条同名都能查到 / 各有独立编号 / 带消歧标签 / 详情返回标签）
- 真实抽取链路（同一段语料连提三次）：
  - 第 1 条"两个刘洋，前端…后端…" → **2 条**：`刘洋[前端]`、`刘洋[后端]`
  - 第 2 条"前端刘洋说…" → 仍是 **2 条**（归一化后命中前端那条）
  - 第 3 条"刘洋今天请假" → 仍是 **2 条**（无标签时归并，不乱建）
- 全接口冒烟：**PASS=36 FAIL=0 SKIP=1**，0 ERROR

### 当时的两个限制（已在下一节修复）

- ~~跨对话的同名无法自动关联~~ → 改为用 `role`（身份线索）+ 别名 + 关系标签多字段打分；
- ~~画像关系解析按姓名取第一条~~ → 改为候选打分（共同标签 / 有画像 / 最近交互）。


### 2026-10-30 — 虚拟角色独立建表（与真人联系人彻底分离）

### 为什么拆

| 维度 | 真人联系人 | 虚拟角色 |
|------|-----------|----------|
| 数据性质 | 从对话**推断**的画像，会过时、要迭代 | **原著设定**，写死的事实，不该被模型改写 |
| 基数 | 几十个 | 可能上千（神话→历史→番剧） |
| 同名 | 罕见，同名即同一人 | 常态（雅典娜 / 雅典娜），必须带作品区分 |
| 生命周期 | 久不联系会衰减、归档 | 不存在"衰减"，除非用户删除 |

共用 `contact_index` 的直接后果（都已复现）：
- `UNIQUE(name, owner_id, owner_type)` 让「雅典娜（希腊神话）」和「雅典娜（圣斗士）」**不能共存**；
- 热表 50 条裁剪（`get_active_contacts`）**不区分** `is_virtual`，角色会占满名额把真人挤进冷表；
- 全项目 **45 处 `is_virtual` 特判**，散落在 11 个文件里。

### 新结构

```sql
virtual_character(id, name, source_work, canon, persona, category,
                  is_builtin, is_archived, owner_type, owner_id, created_at, updated_at)
  UNIQUE(name, IFNULL(source_work,''), owner_id, owner_type)   -- 同名不同作品可共存

virtual_character_alias(id, character_id, alias_name, source_work, alias_type, language)
event_character_link(event_id, character_id)                    -- 事件↔角色，不复用 contact_ids
```

- `canon` = 原著设定（不可改写）；`persona` = 扮演用的人设；
- `contact_index` 只剩真人，`is_virtual` 列保留但恒为 0（历史字段，逐步清理）；
- 对话里提到角色时：不建真人联系人，写入 `event_character_link`；
- `GET/POST/PUT/DELETE /api/virtual_entities` 全部改为读写新表；
- 内置角色（11 个：林克/ChatGPT/马里奥/皮卡丘/孙悟空…）随启动播种到 `system` 名下。

### 数据

当前库里的联系人全部是测试数据，**未做迁移**：直接清空 `contact_index` /
`event_index` / `todo_list` 后重建，新表由 `init_schema` 自动创建。

### 验收

- `scripts/api_acceptance.py`（联系人/角色专项）：**PASS=19 / FAIL=0**
  （含"同名不同作品可共存 雅典娜×2"、"角色不在联系人表"、"角色详情不含 contact 结构"）
- `scripts/api_smoke_test.sh`（全接口）：**PASS=36 / FAIL=0 / SKIP=1**，0 ERROR
- 单元测试：**85 项全过**（`test_write_contacts_skips_virtual_entity` 改为走新表）

### 2026-10-30 — 联系人冷热两级检索 + 虚拟角色独立列表 + 姓名过滤兼容虚拟角色

### 1. 冷热两级检索（原设计有、实现漏了）

产品约定：热表只留常用联系人（外层按 50 条上限裁剪），查询优先热表，**没命中再查冷表**。

实际情况：`search_contacts_with_owner` 只查 `is_archived = 0`，
**没有回退冷表** —— 联系人被热表裁剪归档后就"搜不到了"。

修复：拆成两段查询。先查热表；结果不足 `limit` 时再查冷表补齐（按 id 去重）。
`include_archived=true` 时返回热+冷全量。实测：把"张伟"手工置为归档后，
普通检索**仍能命中**（走冷表回退）。

### 2. 虚拟角色与普通联系人分开

虚拟角色（历史人物 / 动漫 / 科幻角色）与真人共用 `contact_index` 表，
靠 `is_virtual` 区分；但检索接口此前**不区分**，虚拟角色会混进普通联系人结果。

修复：`POST /api/memory/search/contacts` 新增 `include_virtual`（默认 **false**），
普通列表不再包含虚拟角色；虚拟角色统一走 `/api/virtual_entities`。
两套列表互不干扰，物理上仍是一张表（不复制数据，靠标记区分）。

### 3. 姓名过滤器兼容虚拟角色与英文名

之前的三条规则会误杀合法实体名：

| 规则 | 误杀例子 | 修正 |
|------|----------|------|
| 长度 > 6 一律丢弃 | `ChatGPT`、`Alexander`、`哆啦A梦` | 改为只按 **CJK 字数** 判断（>6 才算超长），纯拉丁名放宽到 24 字符 |
| 含虚词/代词一律丢弃 | `初音未来`（含"来"）、`如来` | 改为只看**开头**是否为代词/介词（"由我负责""她以前"仍会被拦） |

新增单测 `keeps_virtual_and_latin_names`：孙悟空 / 曹操 / 林黛玉 / 哆啦A梦 / 初音未来 /
福尔摩斯 / 哈利波特 / ChatGPT / Alexander / T-800 全部保留。

> 虚拟角色走 `POST /api/virtual_entities` 直接落库，本就不经过姓名过滤器；
> 本次修的是"对话里提到虚拟角色时"被误判、以及英文名被误杀的情况。

### 验收结果

- `scripts/api_acceptance.py`（新增，联系人专项）：**PASS=16 / FAIL=0**
  （热表命中、归档后回退冷表、include_archived、虚拟角色创建、默认不混入、
  include_virtual 可见、虚拟列表隔离、详情三库联查、记忆含本人姓名、is_virtual 正反判定）
- `scripts/api_smoke_test.sh`（全接口）：**PASS=36 / FAIL=0 / SKIP=1**，服务全程存活，日志 0 ERROR

### 2026-10-30 — 画像补齐语义修正 + 分词器定案 + 日常可用性复测

### 画像"补不齐"的根因与修复

| 问题 | 旧行为 | 新行为 |
|------|--------|--------|
| 老联系人永远补不上 | 选人条件带 `last_interaction_at >= date('now','-7 days')`，超过 7 天没被提到、又没画像的人被**永久跳过** | 去掉时间窗口：**没画像的一定补**（排序优先），归档的也补（优先级最低） |
| 已有画像的人不会更新 | 只选 `background` 为空的，导致"早一次晚一次"根本不会发生 | 增加条件：`last_interaction_at > 上次画像时间` → 有新互动才更新 |
| 今天没被提到还在白跑模型 | 只要空画像就一直试 | 新增**证据闸**：既无事件、也无任何对话提到他 → 直接跳过，**不记尝试、不占额度** |
| "每天 2 次"被当成硬限制 | 常量写死，批量导入当天大量人排不上队 | 改为 `[heartbeat] profile_max_updates_per_day`（默认 2，批量导入可调大），语义是**资源节奏**而非永久上限 |

### 分词器定案（P0 收敛）

- 崩溃的真实边界是**单次送进分词器的字符数**，不是提示词总长：
  512 字符的中文段仍会触发 MSVC `std::regex` 栈溢出，**128 字符才稳定**。
- 恢复模型自己的 `[INST]` 模板（指令跟随必须靠它），配合 128 字符分段后
  **既不崩、JSON 也能解析**（实测 6 分钟：画像 43→45，解析失败 0，崩溃 0）。
  之前"纯文本提示"虽然不崩，但模型写不出合规 JSON（14 次解析失败）。
- 画像 JSON 解析前先做**括号配平**，修复小模型输出被截断（少 `}`）导致的整条白跑。

### 日常可用性复测

- `bash scripts/api_smoke_test.sh`（默认 `config.toml` + gemma 模型，独立数据目录）：
  **PASS=36 / FAIL=0 / SKIP=1**，服务全程存活。
- 报警扫描（242 条压测 + 画像批处理 + 冒烟）：**ERROR 11 条**，其中
  6 条是"崩溃超限放弃任务"的护栏日志（旧崩溃阶段产生）、2 条是冒烟脚本**故意的负向用例**
  （查询不存在的问题/合并记录），无未处理异常；**无 panic**。
- 唯一非预期告警：`应用模型对话模板失败`（gemma 模板缓冲不足）→ 已自动回退到内置
  gemma 模板，功能不受影响。

### 2026-10-29 — 242 条验收压测：修 6 个缺陷 + 联系人全息详情 + 进程级加固

用 242 条自造语料（短内容 / 长内容 / 数字公式 / 复杂人物关系 / 多人对话 / **联系人专项** / 边界输入）
做验收前压测，全部走真实链路（Ministral-3B、纯 CPU、独立数据目录 `data_stress2`）。
完整报告见 **[docs/STRESS_TEST.md](docs/STRESS_TEST.md)**。

### 压测结果

- 抽取 **242/242 完成，0 失败、0 崩溃**（耗时约 2 小时 40 分，含一次设备断电重启）；
- 联系人 **111 条**，人名槽位召回 **386/393 = 98.2%**，**凭空捏造 0 条**；
- 事件 101、待办 30；
- 人设 23 条（结构化 JSON 100%），其中 17 条带关联联系人、20 条带关系标签；
- 虚拟人物区分、联系人详情三库联查全部通过。

### 压测暴露并修复的问题

| 级别 | 问题 | 真实表现 | 修复 |
|------|------|----------|------|
| **P0** | llama.cpp 分词正则崩服务 | 含超长数字串/无标点长句/重复字符/emoji 的输入触发 `regex_error(error_stack)`；C++ 异常跨 FFI 抛回 Rust 无法捕获 → `fatal runtime error` → **整个进程 abort**。第一轮 200 条语料崩 4 次，每次中断 50+ 条排队任务 | ① 病态输入识别（数字占比>50% / 同字符连续>100 / 无标点>400 字符）→ 跳过 LLM 走正则兜底；② 提示词字符**白名单**（只放行 CJK/ASCII/常用标点，其余丢弃）；③ 每 400 字符软换行；④ 512 字符分段分词；⑤ 线程栈 2 MB → 32 MB；⑥ `SHOUXIN_SKIP_CHAT_TEMPLATE=1` 规避模板内部整段分词 |
| **P1** | 崩溃导致任务无限重试 | 进程 abort 不走普通失败计数，重启后同一条"毒输入"再次崩溃，队列永远推不动 | 恢复任务时 `retry_count += 1` 并写回，超过 3 次直接放弃该条 |
| **P1** | 归档联系人导致整条对话抽取失败 | 活跃联系人超过 50 条被归档后，对话再提到他们时去重查询过滤了 `is_archived=0` → 查不到 → 新 UUID 插入 → 撞唯一索引 `(name,owner_id,owner_type)` → 整条对话的 contacts/events/todos **全部回滚** | 去重包含归档行；写入时自动复活；唯一冲突再按姓名归并重试一次 |
| **P1** | 心跳重复抽取全部历史对话 | `process_new_conversations` 从 1970 年拉全量对话重跑 LLM，与后台队列完全重复（实测一启动就重跑 144 条），还会造重复事件 | 后台队列存在时心跳不再做对话抽取 |
| **P1** | 歧义检测提示词 47k 字符 | 把 30 段对话全量拼进一个提示词，远超 4096 上下文，每轮白跑 30–90 秒 | 从最新往前取，累计 3000 字符为止 |
| **P2** | 关系短语/组织名被当人名 | "小明的姐姐""客户的工程师""外包团队"进联系人表（逐字校验拦不住） | `is_relational_phrase()`：含"的"、含"和/与"且≥4 字、>6 字、纯身份词一律丢弃；并在 MemoryWriter 写入层再加一道闸（事件里的 `related_person_ids` 会绕过抽取层过滤） |
| **P2** | 默认截止日按 UTC 算 | UTC+8 的 00:00–08:00 之间，"7 天后"算成 6 天后 | 新增 `default_deadline_in_days()`，按本地日期算 23:59:59 |

### 性能优化（不改语义）

- **人名校验缓存**：同一次运行内同名只问模型一次，省掉约 40% 的重复推理；
- **压测脚本状态文件按用户隔离**：此前复用同一个 state 文件，导致"断点续传"把上一轮
  `conversation_id` 当成本轮的，200 条被静默跳过、只提交了 42 条；
- **`scripts/run_supervised.sh`**：崩溃自动重启 + 连续秒退保护。

### 新增能力

- **`GET /api/contacts/:id/detail`**：一次返回人设 + 关联联系人（含标签）+ 关联事件 + 相关记忆原文，
  记忆按姓名/别名逐字命中（有界扫描 500 条），不是向量召回；
- **`is_virtual` 进入联系人检索与详情**，虚拟角色与真实联系人共用同一套接口；
- **虚拟角色支持 `user_id`**：此前 `owner_id` 写死 `default_user`，多用户场景下创建完就查不到。

### 新增工具

- `scripts/stress_200.py`：242 条语料生成 + 提交（限流退避）+ 轮询 + 四项校验；
- `scripts/gen_personas.py`：按轮驱动心跳批量生成画像；
- `scripts/run_supervised.sh`：进程守护；
- `tests/test_contact_detail.rs`：联系人详情三库取数的回归测试。

---

### 2026-10-29 — 真实场景跑通测试：修 6 个问题 + 文档重写 + Logo

用"产品经理小林的一天"（4 段对话：晨会 / 客户沟通 / 个人事务 / 技术知识）做端到端实测，
逐项核对抽取、检索、待办、文件、心跳、重启持久化与异常输入。

### 实测发现并修复

| 编号 | 问题（真实表现） | 根因 | 修复 |
|------|------------------|------|------|
| E1 | "月底前出一版方案" 的截止时间变成**当天** | 截止时间取句用"单字命中数"打分，中文常用字几乎人人命中 → 挑错句子 | 改为**覆盖率**（标题字符被句子覆盖 ≥50%），且只在该句/紧邻下一句找时间 |
| E2 | "买维生素" 被安上上一句的"下周一" | 指定句无时间时会**跨句借用** | 匹配句存在时不再跨句回退（宁可回退默认 +7 天）；无匹配句才保留旧的全句扫描 |
| E3 | "月底 / 月初" 解析不出来 | 正则匹配了但解析器不支持 | 新增 `parse_month_edge`（含 12 月跨年） |
| E4 | 5 个互不相关的新事件全被判"相似 0.85"，**一条都没落库** | 合并阈值 0.75/0.90 是按别的嵌入模型定的；e5-base 对无关中文短句本来就有 0.84–0.89 | 阈值改为可配置并校准为 **0.96 / 0.90**（`[extraction]`），实测后事件正常落库 |
| E5 | 事件时间全是"提取当天"（下周三的评审记成今天） | `event_parsed_time` 算出来但**没被使用**（三处写入代码同病） | 统一改用 `event_parsed_time.unwrap_or(now)` |
| E6 | 3.5 万字对话被切成 **175 块**，每块一次 LLM 调用（CPU 上≈1 小时） | 分段合并条件是 `< 80 字符`，且块数无上限 | 改为目标 1200 字符/块、**最多 8 块**，超长部分告警（原文仍在字典库）；实测 142 秒完成 |
| E7 | 联系人校验把整段对话塞进 prompt，超上下文后必然失败并重试 | 未截断 | 校验只取对话前 1500 字符 |
| E8 | 空对话被接受，写入无意义任务 | 缺少非空校验 | 空/纯空白返回 400 |
| E9 | 数据目录硬编码（`./data`、`.sled_data`），配置里的 `data_dir` 被忽略 | 配置加载晚于存储初始化 | 重排初始化顺序，新增 `--data-dir`，支持多实例隔离与备份 |

### 文档

- **README 重写开头**：不再用开发机跑分当标准，改为「三种运行方式 + 三档模型的资源需求」；
  明确标注"想知道自己机器能跑多快，请自己 benchmark"；
- 新增 **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)**：配置覆盖顺序、
  **gguf / Ollama / 云端 API 三种后端切换**、**`context_size` 调节与内存换算**、
  事件阈值校准方法、心跳与待办语义、常用组合示例；
- `docs/PERFORMANCE.md` 的开发机数据明确标注为「仅用于回归对比，不作为选型标准」。

### 品牌

- 新增项目 Logo（`assets/logo.png` 512×512、`assets/logo-128.png`），置于 README 顶部。

### 验证（真实场景，Ministral-3B / 纯 CPU）

- 抽取：4 段对话 → 3 联系人 / 3 待办 / 4 事件，截止时间全部正确（周五=10-30、月底=10-31、默认=11-04）
- 事件时间：评审=11-04、体检=11-02（原文"下周三/下周一"正确解析）
- 语义检索："缓存 key 过期策略" → 命中 0.842；联系人检索、历史检索、智能检索正常
- 待办：完成 / 列表 / 删除正常；文件：上传 / 取文本 / 越权拒绝正常
- 重启持久化：联系人 3 / 待办 2 / 事件 4 全部保留
- 异常：空对话 400、缺字段/非法 JSON 有明确报错、3.5 万字输入 142 秒完成
- `cargo test`：74 单测 + 91 集成 全通过；API 冒烟 35/35

---

## [0.5.0] 2026-10-29 — 心跳审计、磁盘 I/O 优化、开源协议与仓库开放

### 心跳模块审计（12 项维护动作逐条验证）

| 能力 | 结论 | 验证方式 |
|------|------|----------|
| 过期待办失效 | ✅ | 建 `deadline=2026-10-01` 的待办 → 心跳后状态变 `auto_invalid`，日志 `Invalidated 1 expired todos` |
| 待办超量归档 | ✅（按设计只归档陈旧项） | 75 条 pending 时心跳归档 23 条，日志 `Archived 23 stale todos` |
| 待办提醒 | ✅ | pending 超阈值时生成 `⚠️ 你有 N 个待办未处理`，写入侧与心跳侧都有兜底 |
| 联系人画像/关系 | ✅ | 见上一版 C1–C7 修复，实测 tags=['同事'/'上级']、related 反解为姓名 |
| 事件合并待确认 | ✅ | `/api/events/pending_merges` 有数据，过期项由心跳清理 |
| 自适应频率 | ✅ | 日志 `Next heartbeat in 120s (adaptive=true)` |
| 内存保护 | ✅ | 日志 `available memory = 41824 MB (threshold: 4096 MB)` |
| 仅维护模式 | ✅ | `{"maintenance_only":true}` 跳过 LLM，耗时 2–4 ms |
| 并发保护 | ✅ | 重复触发返回 `Heartbeat is already executing` |

**审计中发现并修复**

- `POST /api/todos` 把 `deadline` 当必填字段，导致**无截止时间的待办根本建不出来**
  → 改为可选，缺省取"7 天后 23:59:59"（与抽取链路一致）。
- 曾尝试让"超过上限即归档最旧批次"，但 `tests/test_todo_hot_cold_archive.rs`
  明确约束"**新鲜待办不得被自动隐藏**"，已回退并保留年龄门槛；
  在代码注释与配置注释里写清了这条语义，避免后来者再改错。

### 磁盘 I/O（保护硬盘寿命）

审计到 5 处热点，全部修复：

| 位置 | 问题 | 处理 | 效果 |
|------|------|------|------|
| `heartbeat/executor.rs` 孤儿向量清理 | 每轮心跳（60–120s）**全量读取整个向量库** | 限流为每 6 小时一次；`Vec::contains` 改 `HashSet` | 3 次维护心跳合计读 112 KB |
| `stores/dictionary/store.rs` 时间索引 | `iter()` 从头全量扫描 | 改 `range(since..)` 直接定位 | 心跳不再扫全表 |
| 同上：画像取证最近对话 | 无上限倒扫，owner 对话少时退化为整库遍历 | 最多扫 500 条 | 有界 |
| `stores/vector/store.rs` | **每条记忆 insert 都 `flush()`（fsync）** | 去掉逐条 flush，交给 sled 周期落盘 + 关闭时 flush | 写放大从 N 次同步写降为周期批量 |
| `stores/vector/hnsw_store.rs` | `flush_every_ms(1000)` | 放宽到 5000 ms | 落盘频率降 80% |

**实测**（Ministral-3B 配置、空转 120 秒、无请求）：

```
读 244 KB（61 次） / 写 21.5 KB（22 次）  →  约 122 KB/min 读、10.7 KB/min 写
```

按此折算约 **0.5 GB/月写入**，对 SSD/HDD 寿命影响可忽略。

### 开源协议与仓库

- 新增 **PolyForm Noncommercial License 1.0.0**（禁止商业用途），附 `Required Notice`；
- `Cargo.toml` 补齐 `license` / `description` / `repository` / `readme`；
- 仓库由私有转为**公开**；
- README 开头明确三项预期管理：**面向深度本地智能体用户**、**重代码项目**、**先算物理资源账**。

### 验证

- `cargo test --features gguf --lib`：74 passed
- `cargo test --features gguf --tests`：91 passed（0 failed）
- `bash scripts/api_smoke_test.sh`：PASS=35 FAIL=0 SKIP=0

---

## [未发布] 2026-10-29 — 仓库审计：绝对路径清理、配置失效项修复、换行符规范化

对 GitHub 推送结果、绝对路径、配置文件做了全量审计，修掉 3 类实质问题。

### 1. 绝对路径（仓库可移植性）

| 文件 | 问题 | 处理 |
|------|------|------|
| `scripts/build_gguf.sh` | 写死 MSVC 版本号、`/d/2026-ComfyUI-.../cmake.exe`、甚至 **`E:/ShouXin/...` 项目绝对路径** | 重写为仅做编排，工具链全部交给 `dev_env.sh` 探测 |
| `scripts/dev_env.sh` | 同上 | 改为自动探测：`vswhere` / 扫 VS 安装目录 / SDK 取最新 / cmake 支持 PATH·标准目录·VS 自带·pip；支持 `SHOUXIN_MSVC_ROOT` 等环境变量覆盖 |
| `scripts/cmake_probe.sh` | 写死 crate 版本、构建目录、编译器路径 | 改为复用 `dev_env.sh`，版本从 `Cargo.lock` 读取，构建目录自建 |
| `examples/hnsw_test.rs` | 写死 `D://OpenClaw-Workspace//...//.fastembed_cache` | 改为读 `FASTEMBED_CACHE_DIR`，缺省用 `CARGO_MANIFEST_DIR/.fastembed_cache` |
| `.cargo/config.toml` | 写死本机 CUDA `v12.8` 的 `-L` 与 `CUDA_PATH` | 默认注释掉，改为环境变量方案（默认纯 CPU 构建本就不需要） |
| `src/llm/gguf.rs` | CUDA 探测写死 `C://Program Files\...` | 改用 `ProgramFiles` 环境变量拼接 |

新增 `scripts/toolchain.local.sh.example`：工具装在非标准位置时，复制为
`scripts/toolchain.local.sh`（已 gitignore）填写即可，**仓库内不出现机器路径**。

### 2. 配置失效项（写了不生效）

| 项 | 问题 | 处理 |
|----|------|------|
| `[server]` host/port | **整段未被代码读取**，实际只认命令行 `--host` | 新增 `ServerSettings` 并接入：`--host` 优先，其次配置 |
| `[server].internal_api_token` | 与 `[agent].internal_api_token` 重复，且代码只读后者 | 删除重复项并加注释说明 |
| `[llm.gguf].offload_kv_cache` | 结构体无此字段，`factory` 也未透传 → 恒为 false | 补字段并透传到客户端，配置真正生效 |
| `[embedder].backend` / `hash_dimension` | 文档里有、配置里没有，全靠默认值 | 显式写入并加注释 |
| `[llm.api].model = "gpt-3.5-turbo"` | 过时示例 | 改为 `gpt-4o-mini` 并注明是示例 |
| 重复/矛盾注释 | `[llm.gguf]` 有两条重复的 CPU 说明 | 清理 |

新增 `scripts/check_config.py`：校验 TOML 语法 + **死配置**（配置里有、结构体没有）
+ 漏配置提示。当前结果：`config.toml` / `config.3b.toml` 均 0 死配置。

### 3. 换行符（导致脚本完全无法执行）

Windows 下用文本模式批量改文件，把多个脚本写成了 **CRLF**，
表现为 `bash scripts/build_gguf.sh` **无任何输出、退出码 2**。

- 新增 `.gitattributes`：`*.sh / *.py / *.rs / *.toml / *.md` 强制 `eol=lf`，二进制按 binary 处理；
- 仓库 `core.autocrlf=false`、`core.eol=lf`；
- 全仓 43 个文本文件归一化为 LF；
- 排障文档新增「脚本没输出、退出码 2」一节。

### 验证

- `cargo test --features gguf --lib`：**74 passed**
- `cargo test --features gguf --tests`：**165 passed**（0 failed）
- `bash scripts/api_smoke_test.sh`：**PASS=35 FAIL=0 SKIP=0**
- `python scripts/check_config.py config.toml config.3b.toml`：**0 死配置**
- `[server]` 生效验证：不传 `--host` 时监听 `127.0.0.1:3000`；
  `SHOUXIN__SERVER__PORT=3123` 时监听 `:3123`
- `bash scripts/build_gguf.sh`：自动探测到 MSVC 14.44 + SDK 10.0.26100，构建成功

---

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
