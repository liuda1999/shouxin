# 守心 ShouXin v0.7.0 第三轮测试报告（全新语料）

| 项 | 内容 |
|----|------|
| 测试对象 | shouxin-server **v0.7.0**（commit `32b8568`） |
| 测试时间 | 2026-11-01 04:00–04:30（GMT+8） |
| 数据目录 | `data_v070/`（全新创建，未使用前两轮任何数据） |
| 测试方式 | 全链路黑盒：真实时间戳导入 → 等待真实模型抽取 → 用自然语言回忆；文件走真实上传/嵌入；关键结论均有接口或数据库证据 |
| 语料 | 一个人物 7 个月的生活/工作时间线（20 条，2026-04-12 → 2026-10-31），另有 3 个真实文件 |
| 结论 | 上轮 9 项修复中 **6 项确认修好、3 项部分修好**；本轮新发现 **6 个 P1、7 个 P2**，集中在"历史时间基准""检索半实现""文件融合"三处 |

> 与前两轮的区别：本轮不复用任何旧语料、旧数据库、旧结论。人物、行业、事件全部重新设计，问法也不含原句关键词。

---

## 一、结论速览

### 上轮问题的复验

| 编号 | 上轮问题 | 本轮结果 | 证据 |
|---|---|---|---|
| X-02 | `POST /api/todos` 忽略 query 的 user_id | ✅ **已修** | `?user_id=probe_q` 创建，返回 `owner_id=probe_q`，`default_user` 中无此条 |
| X-04 | 历史检索无 query、limit 无效、只返回最旧 100 条 | ⚠️ **部分修好** | `query`/`limit`/倒序都生效了，但 `limit` 同时充当"搜索深度"：`limit=2` 时第 5 条匹配返回 0 条（见 N-04） |
| X-05 | "12月20号"被记成当天 | ⚠️ **部分修好** | `12月20号` → 2026-12-20 ✅；`十二月二十号` → +7 天兜底 ❌（见 N-02） |
| X-06 | 文件分块无单块上限 | ✅ **已修** | 2MB 无空行文本切成 1758 块，每块约 1200 字（日志按字节显示 3567–3588） |
| X-07 | 文件失败不落状态、不删临时文件 | ✅ **已修** | 损坏 docx → `status=failed` + 完整错误；`temp_uploads/` 残留 0 |
| X-08 | 待办 contact_id 悬空 | ✅ **已修** | 10 个引用全部可解析，且指向正确的人（写复盘→陈立、还钱→郑凯、丝巾→苏蔓） |
| X-11 | 推理超时硬编码 180s | ✅ **静态确认** | `GgufClient.timeout_secs` 已由 `[llm].timeout_secs` 透传（30–3600 钳制） |
| v0.7.0 | 整进程 CPU ≤60% | ⚠️ **近似达成** | 60 个样本：均值 **60.4%**、中位 61.1%、P95 **65.8%**、峰值 **66.9%**（与"2 秒窗口可到 65%"的说明一致，但不是硬 60%） |
| X-10/X-12/X-13/X-14/X-19/X-20/X-21/X-22 | 上轮记录未修 | 仍然存在 | 本轮实测到 X-10 的连带效应（见 N-10），未逐条复测 |

### 本轮新发现

| 编号 | 级别 | 问题 | 一句话后果 |
|---|---|---|---|
| N-01 | P1 | 历史导入的时间基准在抽取链路丢失 | 7 个月前的对话，事件/待办/画像全部按"导入这一刻"算时间 |
| N-02 | P1 | 中文数字日期不解析 | "十二月二十号"落到 +7 天兜底，和"12月20号"差 42 天 |
| N-03 | P1 | 待合并事件的 `merge`/`create` 只改状态 | 返回 `resolved:true`，事件既没合并也没新建，源摘要静默丢失 |
| N-04 | P1 | 历史检索的 `limit` 兼作搜索深度 | `limit=2` 时，第 5 条之后的匹配直接搜不到 |
| N-05 | P1 | 事件检索的 `event_type`/`query`/`owner_type` 被静默忽略 | 传任意 `event_type` 都返回全部 13 条；`event_type` 字段存的还是整段描述 |
| N-06 | P1 | 文件上传默认 `scope=agent` 却接受任意 `user_id` | 形成 (Agent, user) 组合，向量对任何 scope 都不可见 |
| N-07 | P2 | 融合检索对中文仍是"整句关键词" | `matched_terms` = 整句话；contacts/todos 在自然语言问法下恒为 0 |
| N-08 | P2 | 文件内容进不了融合检索 | 语义兜底只回源对话；文件只在 `source_types=["file_chunk"]` 时可见 |
| N-09 | P2 | 虚拟角色与联系人体系断裂 | 创建成功，但详情 404、联系人检索 0，`include_virtual` 无效 |
| N-10 | P2 | 文件嵌入会拖慢对话抽取 | 2MB 文件嵌入期间，普通对话抽取 131.5s（基线 15–20s） |
| N-11 | P2 | "全量导出"与配置项名不副实 | 导出只有联系人/事件/待办；`relational_db_file`、日志配置均不生效 |
| N-12 | P2 | 语义兜底只对事件返回来源与相似度 | history 兜底结果没有 `similarity`/`source`，调用方无法判断可信度 |
| N-13 | P2 | 画像关系标签错误 | 妻子苏蔓被标成"朋友"；"高远"被抽成"高远同行" |

---

## 二、测试数据（本轮全新构造）

### 2.1 人物时间线（20 条，真实历史时间戳）

人物设定：林哲，34 岁，杭州 SaaS 公司产品负责人。语料覆盖入职、项目上线、借钱、体检、婚礼、出差、租房、跳槽、女儿入学、线上事故、旅行、母亲住院、同事离职、生日、复盘。

| 时间 | 内容摘要 |
|---|---|
| 2026-04-12 | 入职云枢科技，汇报赵启明，负责企业版工作台 |
| 2026-04-20 | 女儿小满九月上小学，准备户口本/疫苗本 |
| 2026-05-08 | 工作台上线，陈立盯后端，周雨桐做设计 |
| 2026-05-15 | 郑凯周转 8 万，三个月后还 |
| 2026-06-02 | 体检查出血脂偏高，王医生建议三个月后复查 |
| 2026-06-18 | 表妹下月宁波办喜事，随份子两千 |
| 2026-07-05 | 下周一去深圳拜访客户，高远同行 |
| 2026-07-22 | 滨江两居月租 8500，押一付三 |
| 2026-08-10 | 猎头推荐字节岗位，薪资涨三成 |
| 2026-08-25 | 拒绝 offer，赵启明涨薪并承诺新线 |
| 2026-09-01 | 小满第一天上学，在校门口哭 |
| 2026-09-12 | 刹车片磨损，4S 报价 2800 |
| 2026-09-28 | 线上事故，订单重复扣款 317 单 |
| 2026-10-06 | 国庆厦门行，住曾厝垵 |
| 2026-10-15 | 郑凯说月底还钱 |
| 2026-10-20 | 赵启明要求十二月二十号前交明年预算 |
| 2026-10-24 | 母亲宁波住院，胆结石 |
| 2026-10-28 | 周雨桐提离职，要招设计师 |
| 2026-10-30 | 下周三苏蔓生日，想要蓝色丝巾 |
| 2026-10-31 | 复盘会定三条改进：支付幂等、对账告警、事故演练 |

### 2.2 文件

| 文件 | 大小 | 用途 |
|---|---|---|
| `Q3复盘会纪要.md` | 1.7 KB | 真实会议纪要，含灰度比例、回滚阈值、招聘 HC、预算日期 |
| `无空行产品说明.txt` | 900 KB | 验证 1200 字切块（无任何空行） |
| `损坏的合同.docx` | 1.2 KB | 验证失败路径的状态与临时文件清理 |
| `大文件压测.txt` | 2 MB | 验证吞吐与 CPU 上限 |

---

## 三、已修项复验详情

### 3.1 写入与抽取：稳定

- 20 条批量导入一次请求全部接收（`accepted:20, failed:0`），单条写入毫秒级返回；
- 20/20 抽取完成，无失败；
- 产出：7 个联系人、13 条事件、14 条待办、2 条待合并事件。

### 3.2 文件链路：这轮最扎实的修复

| 场景 | 结果 |
|---|---|
| 900KB 无空行文本 | `completed`，切块正常（不再 1 块） |
| 2MB 无空行文本 | `completed`，1758 块，10 分 26 秒 |
| 损坏 docx | `status=failed`，错误信息："Word文档ZIP解析失败: invalid Zip archive..." |
| 临时文件 | 成功/失败两条路径均为 0 残留 |
| MD5 去重 | 重复上传返回 `is_duplicate:true` + 原 file_id，不重复嵌入 |

### 3.3 待办与联系人：核心正确性明显提升

- `POST /api/todos?user_id=alice` 正确落到 alice；
- 重复 `PUT /complete` 幂等，`completed_at` 不被改写；
- 10 个 `contact_ids` 全部可解析，且人对应正确：

```
写复盘        -> 陈立
还钱          -> 郑凯
购买蓝色丝巾  -> 苏蔓
复查血脂情况  -> 王医生
提交明年的预算 -> 赵启明
```

### 3.4 语义召回：换词可查，基本兑现

10 个不含原句关键词的问法，`/api/search/smart` 的 history 结果：

| 问法 | 首位命中 | 结论 |
|---|---|---|
| 谁找我借过钱 | 郑凯周转八万 | ✅ |
| 我女儿上学遇到什么麻烦 | 小满校门口哭 | ✅ |
| 车子要修哪里 | 刹车片磨损 | ✅ |
| 身体哪里不好 | 妈妈住院（血脂第 2） | ⚠️ 歧义 |
| 团队里有人要离职吗 | 周雨桐提离职 | ✅ |
| 年底前要交什么材料 | 小学入学（预算第 4） | ⚠️ 排序弱 |
| 国庆去哪玩了 | 厦门 | ✅ |
| 我妈身体怎么了 | 胆结石住院 | ✅ |
| 最近有没有涨薪 | 涨薪+新线 | ✅ |
| 现在房租多少 | 月租 8500 | ✅ |

**首位命中 8/10，前五命中 10/10。** 但请注意：这 10 条全部由 **history 的语义兜底**救回，contacts 与 todos 两类**全部为 0**（见 N-07）。

### 3.5 CPU 上限：接近设计值

2MB 文件嵌入期间采样 60 次（每 2 秒）：

```
均值 60.4% | 中位 61.1% | P95 65.8% | 峰值 66.9%
```

结论：作业对象约束**真实生效**（旧版同负载峰值 94.2%），但不是硬 60%。文档已说明"2 秒窗口可到 65% 左右"，实测略高 1.8pp。要严格不越线，需按文档建议设 55。

---

## 四、新发现详情

### N-01 【P1】历史导入的时间基准在抽取链路丢失

**现象（已确证）**

20 条对话用 `timestamp` 导入了 4–10 月的真实时间，原文时间戳**存得完全正确**：

```
POST /api/memory/search/history  start=2026-04-01 end=2026-05-01
→ 2026-04-20、2026-04-12 两条（时间正确，倒序）
```

但抽取产出全部按"导入这一刻"计算：

| 原文时间 | 原文 | 实际落库 |
|---|---|---|
| 2026-04-12 | 今天入职… | 事件 `time_range_start=2026-10-31` |
| 2026-05-15 | 三个月后还 | 事件 `2027-01-29`（= 现在 +3 月） |
| 2026-06-02 | 三个月后复查 | 事件/待办 `2027-01-29` |
| 2026-07-05 | 下周一去深圳 | 事件 `2026-11-02`（= 现在后的下周一） |

13 条事件里 **9 条落在导入当天**，另外 3 条按"现在"推算（7 月说的"下周一"→11-02、两条"三个月后"→2027-01-29），只有 1 条（10-30 说的"下周三"→11-04）恰好正确。

14 条待办里 **9 条挤在 2026-11-08**（+7 天兜底），3 条在 10-31，1 条在 2027-01-29，只有"购买蓝色丝巾"（11-04）与原文一致。

**根因**

`src/service/background_queue.rs:28-36`：

```rust
pub struct ExtractionTask {
    pub conversation_id: String,
    pub conversation_text: String,
    pub owner_type: OwnerType,
    pub owner_id: String,
    pub user_id: Option<String>,
    pub retry_count: u32,
}
```

任务结构里**没有 `occurred_at`**。抽取 worker 只能拿 `Utc::now()` 当基准：

- `background_queue.rs:826-832`：`let now = Utc::now();` → `try_resolve_event_time(..., now)`；
- `background_queue.rs:999`：`resolve_deadline_in_text(...)` 同样以当前时刻为基准；
- 传给 LLM 的抽取调用是 `extractor.extract(&messages)`，也没有对话日期（`background_queue.rs:340`）。

**影响**

- README 宣传的"带历史 `timestamp` 导入旧数据"只完成了"存"，没完成"算"；
- 导入的历史越久，事件时间线、待办截止日、画像 `major_events` 偏离越远；
- 依赖时间的检索（"上个月""今年三月"）全部失准。

**修复建议**

1. `ExtractionTask` 增加 `occurred_at: DateTime<Utc>`，入队时从对话记录带过来（`/api/memory/add` 已经解析了 timestamp，只需透传）。
2. 抽取提示词里注入"这段对话发生在 {occurred_at}，请以此为基准解析相对时间"。
3. 本地兜底解析 `resolve_deadline_in_text` / `try_resolve_event_time` 的 `now` 参数改为 `occurred_at`。
4. 回归用例：导入 2026-05-15 的"三个月后还"，断言事件时间落在 2026-08-15 而不是导入日 +3 月。

---

### N-02 【P1】中文数字日期不解析，静默落到 +7 天

**现象（已确证，A/B 对照）**

同一句话，只差数字写法：

| 输入 | 截止时间 | 结果 |
|---|---|---|
| `提醒我12月20号前把年度预算交给赵启明` | 2026-12-20 23:59 | ✅ |
| `提醒我十二月二十号前把年度预算交给赵启明` | 2026-11-08 23:59 | ❌（+7 天兜底） |

差 42 天，且没有任何"sorry 没解析出来"的提示。

**根因**

`src/utils/time_parser.rs:220-222`：

```rust
static_regex!(re_md, r"(\d{1,2})\s*[月\-/]\s*(\d{1,2})\s*[日号]?");
static_regex!(re_day_only, r"(\d{1,2})\s*[号日]");
```

只认 `\d`。`re_days_later`（`:287`）虽然接受"一二三…"，但要求后面跟"天/周/月/年"，不覆盖"十二月二十号"。

**影响**

中文口语里"十二月二十号""三月十五号"极常见；解析失败后不是留空，而是套用"7 天后"的默认值，用户会以为系统记住了，实际记错。

**修复建议**

1. 在日期扫描前做一次中文数字归一化（`十二月二十号` → `12月20号`），只处理"月/日/号"上下文，避免误伤"三个月"。
2. 归一化失败时**留空并标记** `deadline_unresolved`，不要用 +7 天冒充。
3. 回归用例覆盖：`十二月二十号`、`三月十五日`、`十月一号`、`十一月三十一号`（非法日期应拒绝而不是兜底）。

---

### N-03 【P1】待合并事件的 `merge` / `create` 只改状态，不做合并

**现象（已确证）**

队列里真实的待合并记录（相似度 0.909）：

```
source: 企业版工作台上线，首日 1.2 万单
target: 汇报给赵启明，负责企业版工作台产品线
```

调用 `POST /api/events/pending_merges/1/resolve {"action":"merge"}`：

```
返回：{"success":true,"data":{"resolved":true,"action":"merge"}}
事件总数：13 → 13（不变）
target.summary：无变化
pending 表：status=resolved, action=merge
```

源事件（上线）既没合并进目标，也没落库，**永久丢失**。

再用不存在的 id 试：

```
POST /api/events/pending_merges/999999/resolve {"action":"merge"}
→ 200 {"resolved":true}
```

**根因**

`src/stores/relational/store.rs:3077-3101`：`resolve_pending_event_merge` 只执行一条 `UPDATE ... SET status, resolution_action`，不检查 `rows_affected`，也没有 merge/create 分支。真正会合并的是另一条 `confirm_event_merge`（`store.rs:3400+`），但这条 API 没走它。

**影响**

- 调用方拿到成功，数据却没变化——比报错更糟；
- 被误判为"待合并"的事件（上线）永远不会出现在事件列表里；
- 阈值 0.90 明显偏低：上线 vs 入职汇报 0.909、预算 vs 涨薪 0.919，都是误配。

**修复建议**

1. `resolve` 按 action 分发：`merge` 调 `confirm_event_merge(source, target)`；`create` 把源事件真正插入 `event_index`；`dismiss` 只改状态。
2. 更新语句检查 `rows_affected == 0` → 返回 404。
3. 用本轮语料重新校准 `event_merge_pending_threshold`（0.90 → 至少 0.94），并把"同一个人 + 同一主题"作为前置条件。

---

### N-04 【P1】历史检索的 `limit` 兼作"搜索深度"，小 limit 搜不到旧匹配

**现象（已确证）**

原文里明确有"预算"（2026-10-20），但：

```
query=预算 limit=2  → 0 条
query=预算 limit=3  → 0 条
query=预算 limit=10 → 1 条（2026-10-20）
query=预算 limit=50 → 1 条（2026-10-20）
```

**根因**

`src/api/routes.rs:2121-2145`：

```rust
let limit = req.limit.unwrap_or(50).clamp(1, 200);
...
async_memory.search_history((start, end), scope, user_id, limit)   // 只取最近 limit 条
...
.filter(|h| terms.iter().any(|t| content.contains(t)))
.take(limit)
```

`limit` 既是"取多少条候选"，又是"返回多少条"。目标排在第 5 位时，`limit=2/3` 根本拿不到它。上轮 X-04 的"先截断后过滤"以另一种形式回来了。

**影响**

调用方按常规理解传 `limit=3` 想要"最多 3 条"，实际把搜索范围也砍到 3 条，返回空还以为没记过。

**修复建议**

1. 候选量与返回量解耦：取 `max(limit*20, 200)` 或按时间分段扫描，过滤后再 `take(limit)`。
2. 或直接在字典库层做关键词过滤（`conversation_ts_index` 遍历时判断），避免全量取回内存。
3. 回归：20 条语料中第 5 条命中，`limit=3` 必须返回 1 条。

---

### N-05 【P1】事件检索的三个字段被静默忽略，`event_type` 还不是类型

**现象（已确证）**

```
POST /api/memory/search/events {"event_type":"meeting"}          → 13 条
POST /api/memory/search/events {"event_type":"travel"}           → 13 条
POST /api/memory/search/events {"event_type":"nonexistent_type"} → 13 条
POST /api/memory/search/events {"query":"银行卡冻结"}             → 13 条
```

**根因**

1. `routes.rs:208` 的 `event_type` 字段在 handler 中**从未读取**；`EventFilter` 也没有 `event_type` 字段，两条 SQL 都没有 `event_type = ?`。
2. `routes.rs:199-237` 的 `SearchEventsRequest` 没有 `query` / `owner_type` 字段，`#[derive(Deserialize)]` 未开 `deny_unknown_fields`，按 `docs/API.md:249` 的示例传 `query` 会被 serde 静默丢弃。
3. 更根本的是：`event_type` 列里存的不是类型，而是 LLM 的整段描述（`background_queue.rs:846`：`event_type: llm_description`）：

```
title=汇报给赵启明
event_type=汇报给赵启明，负责企业版工作台这条产品线。公司在滨江，通勤四十分钟。
```

**影响**

"按类型筛选事件"这个能力在数据层就不成立；文档示例写 `query`，调用方以为在搜关键词，实际拿到全量。

**修复建议**

1. 补 `EventFilter.event_type` + SQL 条件；同时把 `event_type` 改成真正的枚举（meeting/travel/accident/…），描述另存 `description`。
2. `SearchEventsRequest` 增加 `query`（映射到 `search_query`）与 `owner_id` 别名，或启用 `deny_unknown_fields` 让错误显式化。
3. 文档与实现二选一对齐。

---

### N-06 【P1】文件上传的默认 scope 与 user_id 组合，产生"任何 scope 都搜不到"的向量

**现象（已确证）**

按最自然的写法上传（只传 `user_id`）：

```
POST /api/files/upload?filename=Q3复盘会纪要.md&user_id=linzhe
→ 文件详情里 "owner_type":"agent", "owner_id":"linzhe"
```

这份文件的文本可以被 `/api/files/:id/text` 读到，但语义检索：

```
scope=user  → 5 条，全是 event/conversation，无 file_chunk
scope=agent → 0 条
```

同一份文件显式加 `scope=user` 重传后，才能用 `source_types=["file_chunk"]` 检到 1 条。

**根因**

- `src/api/routes.rs:578-590`：`default_scope()` 返回 `"agent"`，而 `user_id` 默认 `default_user`；
- `routes.rs:773`：`owner_type: scope.clone()`，于是 (Agent, linzhe) 被写入 `file_index` 和向量元数据；
- 语义检索的 owner 过滤只有 (User, user_id) 与 (Agent, primary_agent_id) 两种合法组合（`three_library_manager.rs:1395-1410`），(Agent, linzhe) 永远不会命中。

**影响**

文件显示 `completed`、正文可读，但"嵌入后可检索"这一承诺对默认参数不成立，且没有任何告警。

**修复建议**

1. 上传时若 `scope` 未显式给，按 `user_id` 是否存在推断：有 user_id → User，否则 Agent。
2. 或强制 `scope` 必填，缺省直接 400。
3. 写入前校验 `(owner_type, owner_id)` 组合合法性，非法组合拒绝入库。

---

### N-07 【P2】融合检索对中文仍是"整句关键词"，结构化类别全空

**现象（已确证）**

10 个自然语言问法里：

```
contacts = 0 / 10
todos    = 0 / 10
matched_terms = ["谁找我借过钱"]        ← 整句话
```

带空格或联系人名才恢复：

```
"郑凯 借钱"      → terms=["郑凯","借钱"]，contacts=1
"赵启明最近怎么样" → terms=["赵启明最近怎么样","赵启明"]，contacts=1
```

**根因**

`routes.rs:2045-2060` 的 `split_query_terms` 只按空白/标点切分，不做中文分词；`expand_terms_with_contacts` 只把"整句里出现的联系人全名"补进去。整句作为一个 term，`LIKE '%整句%'` 在 contacts/todos 上必然 0 命中。

**影响**

- `matched_terms` 对中文基本没有信息量；
- contacts/todos 两条召回路径在自然语言下等于不存在，全靠 history 的向量兜底；
- 若嵌入模型缺失（`fallback_to_hash=true`），兜底失效，整体召回会瞬间归零。

**修复建议**

1. 引入轻量中文分词（jieba-rs 或 n-gram 倒排），至少对 contacts/todos 做 2-gram 召回。
2. `matched_terms` 返回真正参与匹配的词，而不是整句。
3. 给 contacts/todos 补语义兜底（现在只有 events/history 有）。

---

### N-08 【P2】文件内容进不了融合检索

**现象（已确证）**

问题"灰度比例是多少"的答案只存在于上传的会议纪要里：

```
/api/search/smart        → contacts 0 / events 4 / history 8 / todos 0（无文件）
/api/memory/search/semantic（无过滤）→ top-8 无 file_chunk
/api/memory/search/semantic（source_types=["file_chunk"]）→ 1 条，命中
```

另外 `docs/API.md:142` 示例的 `source_types` 是 `["event","learning"]`，而文件实际的 `material_type` 是 `file_chunk`，按文档值过滤永远 0。

**根因**

- `routes.rs:2778-2810` 的 history 语义兜底要求 `source_conversation_id`，文件块没有，直接 `continue`；
- 没有 file 类型的独立返回槽位；
- `file_processor.rs:126` 写的是 `material_type="file_chunk"`，文档未同步。

**修复建议**

1. `SmartSearchResponse` 增加 `files` 字段，语义兜底接受 `file_chunk` 并回源文件名/块序号。
2. 统一 `material_type` 命名（`file` 或 `file_chunk`），文档同步。
3. 文件检索按 `source_file_id` 聚合，同一文件只返回最相关的一块。

---

### N-09 【P2】虚拟角色与联系人体系断裂

**现象（已确证）**

```
POST /api/virtual_entities {"name":"姚广孝",...} → 创建成功
GET  /api/contacts/{entity_id}/detail              → 404
POST /api/memory/search/contacts {"query":"姚广孝","include_virtual":true} → 0 条
```

**根因**

`virtual_entity.rs:274` 只写 `virtual_character` / `virtual_character_alias`，不写 `contact_index`；而 `search_contacts` / `contact detail` 只查 `contact_index`。`include_virtual` 参数因此是永久空操作。

**文档与实现不一致**：`docs/API.md:440` 称"与真人共用检索/详情接口"，实现已改为"独立表"（writer 注释明确写了不再塞 contact_index），文档未更新。

**修复建议**

1. 二选一：要么文档改成"仅 `/api/virtual_entities` 可查"，要么在 `contact_index` 同步一行 `is_virtual=1`。
2. 删除无用的 `include_virtual` 参数，避免误导。

---

### N-10 【P2】文件嵌入会阻塞/拖慢对话抽取

**现象（已确证）**

2MB 文件处理期间，提交一条普通对话：

```
抽取完成耗时 131.5s
```

同一实例、同一模型，空载时 20 条平均 15–20s。慢 **7–8 倍**。

文件本身吞吐：

```
2MB / 1758 块 / 626 秒 ≈ 2.8 块/秒
按 20MB 上传上限外推 ≈ 1.7 小时
```

**根因（推断，未逐行确认）**

- 文件嵌入在独立任务里逐块 `embed().await`，与 LLM 抽取共享同一嵌入器/CPU 预算；
- 抽取队列仍是单 worker 串行（X-10 未修），文件任务不参与队列调度，两者直接抢 CPU。

**影响**

"上传大文件 + 同时聊天"是常见组合，当前会让对话的实时抽取明显延迟；文件进度也只有 `processing`，没有块级进度。

**修复建议**

1. 文件嵌入降到低优先级队列，或在抽取队列空闲时再跑；
2. 文件处理暴露 `processed_chunks/total_chunks`；
3. 与 X-10 一起做：队列并发度 + 资源配额。

---

### N-11 【P2】"全量导出"只有三张表；三个配置项是死配置

**现象（已确证）**

```
GET /api/export?user_id=linzhe
→ keys: contacts(7), events(13), todos(14), counts, exported_at
```

没有对话原文、没有文件、没有向量。README `:85` 写的是"导出全量 JSON"。

死配置（静态确认）：

| 配置 | 写了什么 | 实际 |
|---|---|---|
| `[storage].relational_db_file` | `relational.db` | 实际文件是 `shouxin.db`（`shouxin-server.rs:407` 硬编码） |
| `[storage].vector_index_file` | `vectors.hnsw` | 无生产读取点 |
| `[logging].dir/level/max_file_size_mb` | `./logs`、`info`、10MB | 日志只走 stdout；`logs/` 目录为空；级别硬编码 INFO |

**修复建议**

1. 导出增加 `conversations`（可按时间范围分页）与 `files` 元数据；或者在文档里改成"结构化索引导出"。
2. 死配置二选一：接上实现，或从 `config.toml`/文档删除。

---

### N-12 【P2】语义兜底只对事件返回 `similarity` / `source`

**现象**

`/api/search/smart` 的 events 兜底结果带有：

```
{"similarity":0.87,"source":"semantic","source_conversation_id":"..."}
```

history 兜底结果直接返回 `Conversation` 对象，没有 `similarity`、没有 `source`。但实际召回 10/10 都来自 history 兜底。

**根因**：`routes.rs:2778-2810` 把 `dict.get_conversation_by_id` 的原对象直接 `to_value`，丢掉相似度。

**建议**：给 history 结果包一层 `{...conversation, similarity, source:"semantic"}`，或在 `Conversation` 上加可选字段。

---

### N-13 【P2】画像与实体抽取的质量问题

- 苏蔓是妻子，`relationship_tags` 却是 `["朋友"]`；
- "高远同行"被当成一个联系人姓名（原文"高远同行"是"高远 + 同行"）；
- 13 条事件漏了"工作台上线""租房""字节 offer""刹车片"等关键事件，但画像文字里又提到了部分内容（与上轮 X-13 同源）。

这类属于提示词/产品调优，不在本轮修复范围，但会影响"联系人档案"的可用性。

---

## 五、"未实现 / 无效"清单（静态审计 + 实测）

| 类别 | 位置 | 问题 | 本轮验证 |
|---|---|---|---|
| 端点半实现 | `store.rs:3077-3101` | `pending_merges/:id/resolve` 的 merge/create 不落地 | ✅ 实测（N-03） |
| 端点半实现 | `routes.rs:1317` | `/api/contacts/:id/event_archive` 忽略 `scope/user_id` | 静态确认 |
| 参数无效 | `routes.rs:208` | `event_type` 从未读取 | ✅ 实测（N-05） |
| 参数无效 | `routes.rs:199-237` | `query`/`owner_type` 不在结构体，静默丢弃 | ✅ 实测（N-05） |
| 参数无效 | `routes.rs:578-590` | 上传 `scope` 默认 agent，与 user_id 组合非法 | ✅ 实测（N-06） |
| 参数无效 | `routes.rs:1317` | `DictionaryQueryParams.scope/user_id` 未使用 | 静态确认 |
| 空实现 | `store.rs:2230` | `RelationalStoreTrait::cleanup()` 恒 `Ok(0)` | 静态确认 |
| 空实现 | `stores/relational/mod.rs:31` | 同名 trait 无任何 impl，纯死代码 | 静态确认 |
| 死配置 | `settings.rs:422-425,443-446` | `relational_dir/db_file/vector_index_file/logging.*` | ✅ 实际 `shouxin.db`、`logs/` 空 |
| 配置不一致 | `internal_api.rs:86-92` | `/internal/*` 只读环境变量 `INTERNAL_API_TOKEN`，不读 `[agent].internal_api_token` | 静态确认 |
| 必填未用 | `internal_api.rs:35` | `source_agent_id` 必填但全程不读 | 静态确认 |
| 文档超前 | `docs/API.md:440` | 虚拟角色"共用检索/详情" | ✅ 实测 404 + 0（N-09） |
| 文档超前 | `README.md:85` | "全量 JSON 导出" | ✅ 实测只有三张表（N-11） |
| 文档滞后 | `docs/API.md:142` | `source_types` 示例不含 `file_chunk` | ✅ 实测（N-08） |

---

## 六、可行性判断：什么能用，什么别指望

**可以放心用的**

1. **写入与存档**：毫秒级写入、批量导入、原文永久保留、时间戳按历史写入。
2. **换词回忆（对话层）**：问法不含原词也能召回，前五命中 10/10；这是 0.6.9 的真实进步。
3. **文件切块与失败处理**：1200 字上限、无空行也能切、失败落状态、临时文件不残留、MD5 去重。
4. **待办的基础操作**：user_id 归属、完成幂等、contact_id 映射全部正确。
5. **备份**：`VACUUM INTO` 快照真实可用（364KB，可打开）。
6. **CPU 预算**：从 94% 压到 ~60%，后台驻留不再吃满整机。

**需要绕开或谨慎用的**

1. **历史数据导入**：时间会漂移（N-01）。如果要导入旧聊天记录，导入后事件/待办的时间基本不可信；**建议先修 N-01 再导入历史**。
2. **按人名/待办做结构化检索**：自然语言问法下 contacts/todos 恒为 0（N-07）。调用方需要自己把问句拆成关键词，或改用 `/api/memory/search/contacts` 精确查。
3. **文件知识检索**：走 `/api/search/smart` 查不到（N-08），要显式调 `/api/memory/search/semantic` 且带 `source_types=["file_chunk"]`，且上传时必须带 `scope=user`（N-06）。
4. **事件按类型筛选**：字段无效（N-05），只能拿全量再在客户端过滤。
5. **待合并事件确认**：merge/create 是假成功（N-03），目前只能 `dismiss`，或改用 `/api/events/confirm_merge`。
6. **虚拟角色**：只能通过 `/api/virtual_entities` 自己管理，不要指望联系人详情/检索能带出来（N-09）。
7. **大文件入库**：2MB 要 10 分钟，20MB 约 1.7 小时，且会拖慢同时段的对话抽取（N-10）。建议限制单文件在几百 KB，或放到空闲时段。

---

## 七、修复优先级建议

| 顺序 | 问题 | 理由 | 预估改动 |
|---|---|---|---|
| 1 | N-01 历史时间基准 | 影响所有导入数据的时间正确性，且会污染画像/待办；改动集中（任务结构 + 3 处 now） | 小 |
| 2 | N-02 中文数字日期 | 中文口语高频，错得静默；归一化即可 | 小 |
| 3 | N-03 合并确认假成功 | 返回成功但丢数据，是最危险的一类接口 | 中 |
| 4 | N-05 事件检索字段 | 文档与实现两张皮，影响所有按类型/关键词查事件的调用方 | 中 |
| 5 | N-04 历史 limit 兼作深度 | 一行候选量放大即可缓解 | 小 |
| 6 | N-06 文件 scope 默认值 | 决定文件检索是否可用 | 小 |
| 7 | N-07 中文分词 | 决定 contacts/todos 能否自然语言召回 | 中 |
| 8 | N-08 文件进融合检索 | 产品承诺"文件可检索"的最后一公里 | 中 |
| 9 | N-10 文件与抽取争抢 | 影响并发体验，与 X-10 一起做 | 中大 |
| 10 | N-11 导出与死配置 | 运维可信度问题 | 小 |

---

## 八、附录：复现命令

### A. 起服务（全新目录）

```bash
cd /e/ShouXin
./target/release/shouxin-server.exe --data-dir ./data_v070
```

### B. 导入真实时间线

```bash
python3 E:/tmp/v070/import.py   # 20 条 batch，带 timestamp
# 2–3 分钟后查抽取状态
```

### C. N-01 证据

```bash
curl -s "localhost:3000/api/memory/search/history" -H 'Content-Type: application/json' \
  -d '{"query":"","limit":5,"user_id":"linzhe","start_time":"2026-04-01T00:00:00+08:00","end_time":"2026-05-01T00:00:00+08:00"}'
# 原文时间正确
curl -s "localhost:3000/api/memory/search/events" -H 'Content-Type: application/json' \
  -d '{"user_id":"linzhe","limit":50}' | python3 -c "import sys,json;[print(e['title'], e['time_range_start']) for e in json.load(sys.stdin)['data']]"
# 事件时间落在 2026-10-31
```

### D. N-02 A/B

```bash
curl -s -X POST localhost:3000/api/memory/add -H 'Content-Type: application/json' \
  -d '{"conversation":"提醒我十二月二十号前把年度预算交给赵启明。","user_id":"cn_num"}'
curl -s -X POST localhost:3000/api/memory/add -H 'Content-Type: application/json' \
  -d '{"conversation":"提醒我12月20号前把年度预算交给赵启明。","user_id":"arabic"}'
# 30s 后对比：cn_num=2026-11-08，arabic=2026-12-20
```

### E. N-03 假成功

```bash
curl -s -X POST localhost:3000/api/events/pending_merges/999999/resolve \
  -H 'Content-Type: application/json' -d '{"action":"merge"}'
# {"success":true,"data":{"resolved":true}}
```

### F. N-05 字段无效

```bash
for t in meeting travel nonexistent; do
  curl -s localhost:3000/api/memory/search/events -H 'Content-Type: application/json' \
    -d "{\"user_id\":\"linzhe\",\"limit\":50,\"event_type\":\"$t\"}" \
    | python3 -c "import sys,json;print('$t', len(json.load(sys.stdin)['data']))"
done
# 三种都返回 13
```

### G. N-06 文件归属

```bash
# 只传 user_id（默认 scope=agent）
curl -s -X POST "localhost:3000/api/files/upload?filename=Q3复盘会纪要.md&user_id=linzhe" \
  -H 'Content-Type: application/octet-stream' --data-binary @Q3复盘会纪要.md

# 语义检索（scope=user）→ 无 file_chunk
curl -s localhost:3000/api/memory/search/semantic -H 'Content-Type: application/json' \
  -d '{"query":"灰度比例是多少","limit":8,"user_id":"linzhe"}'
```

---

*报告结束。所有 P1 结论均在 `data_v070/` + 上述命令下可复现；未触碰仓库既有 `data*/`。*
