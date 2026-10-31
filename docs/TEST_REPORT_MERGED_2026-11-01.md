# 守心 v0.7.0 合并测试报告（第二轮 · 智能体视角）

> 合并对象：`docs/TEST_REPORT_v0.7.0_2026-11-01.md`（第三轮）＋ 本次第二轮实跑
> 测试时间：2026-11-01 04:30–05:40（GMT+8）
> 被测版本：0.7.0（二进制 `target/release/shouxin-server.exe`，构建于 2026-11-01 03:36）
> 数据目录：`data_a2`（全新，未复用任何历史测试数据）
> 本文取代上一份报告的问题清单；上一份的报告结构、方法论仍可参考。

---

## 0. 这一轮是怎么测的

上一轮我是"测试者"，造一个人物、灌 7 个月语料、看它抽得对不对。
这一轮换了身份：**把守心当成我自己的记忆桶**，按 README / ARCHITECTURE / API / CONFIGURATION 四份文档当用户用一遍。

两件事同时做：

1. **真实工作负载**：把这几天做这个项目时真实产生的 15 条工作记录（带真实发生时间、真实数字、真实结论）批量灌进去，然后**假装上下文丢失**，只靠 API 回忆。测的是"这个桶到底能不能替我记住事"。
2. **文档逐条对账**：把四份文档里写明的行为（时间戳三种格式、会话日 04:00 切分、批量 500 上限、待办无字段级更新返回 405、冷表默认可见、虚拟角色共用检索…）逐条当断言跑，看哪些是真的。

语料不是编的：`docs/BUILD.md` 的构建坑、`PERFORMANCE.md` 的 2.8 条/分、第三轮的 1758 块 / 10 分 26 秒 / 131.5 秒、用户"本机不用鉴权"的决定、0.96 vs 0.90 的文档冲突、`liuda1999`、PolyForm 许可证——每条都能在仓库或本会话里找到出处。

---

## 1. 关键问题总榜（按级别排序）

级别定义沿用上一份：**P1 = 核心功能失效 / 数据错误**，**P2 = 功能残缺 / 需绕行**，**P3 = 文档与体验瑕疵**。P0 为空（本地单机威胁模型下，鉴权类问题已在上一份排除）。

| 排序 | 编号 | 级别 | 问题 | 一句话后果 | 状态 |
|---|---|---|---|---|---|
| 1 | N-01 | **P1** | 历史导入的时间基准在抽取链路丢失 | 10-20 说的"下周三"被算成 11-04，历史记忆时间整体错位 | 上轮已报，**仍在** |
| 2 | N-14 | **P1** | 日期表达容错不一致：带空格 / 斜杠 / 中文数字一律静默落到"今天+7" | "11 月 12 号"→11-08、"十一月十二号"→+7，且无任何提示 | **新**（含上轮 N-02） |
| 3 | N-17 | **P1** | "本周X"允许指向已过去的日期，待办创建后立即 `auto_invalid` | 周日说"这周五"，记成 10-30，6 小时后这条待办静默消失 | **新** |
| 4 | N-07 | **P1** | 中文不分词，"分词"只是按空白和标点切 | `matched_terms`=整句话；contacts/todos 在自然语言问句下恒为 0 | 上轮已报，根因定位 |
| 5 | N-18 | **P1** | 语义兜底无相似度阈值、结果不带 `similarity`/`source` | 问"火星殖民计划"也返回 2–3 条无关记忆，调用方无法判断可信度 | 上轮 N-12 升级 |
| 6 | N-05 | **P1** | 事件检索的 `query` / `hot_only` / `archived_month` 被忽略；`event_type` 列存的是描述 | 按类型筛选在数据层就不成立 | 上轮已报，**根因找到** |
| 7 | N-15 | **P1** | 事件 `summary` 列落的是标题副本，`event_type` 列落的是描述 | 所有读 `summary` 的地方只看得到截断标题 | **新** |
| 8 | N-04 | **P1** | 历史检索的 `limit` 兼作"搜索深度" | `query=报告&limit=2`→0 条，`limit=5`→1 条 | 上轮已报，**仍在** |
| 9 | N-06 | **P2** | 上传默认 `scope=agent`，该命名空间没有任何检索入口 | 不带 scope 上传的真实文档，内容入了向量库但谁也搜不到 | 上轮已报，**仍在** |
| 10 | N-19 | **P2** | `GET /api/todos?contact_id=` 过滤无效 | 传任意 contact_id 都返回全部待办 | **新** |
| 11 | N-20 | **P2** | "知识 / facts / learnings"维度未实现 | 21 张表里没有任何知识表，提示词也不要求模型输出 | **新** |
| 12 | N-16 | **P2** | 同一件事生成两条待办，去重失效 | "把…改成必填"与"将…改成必填"并存 | **新** |
| 13 | N-09 | **P2** | 虚拟角色与联系人体系断裂 | 创建成功，但 `/api/contacts/:id/detail` 404、联系检索 0 | 上轮已报，**仍在** |
| 14 | N-11 | **P2** | 导出不含对话原文；`relational_db_file` 等是死配置 | "全量导出"带走不了原文 | 上轮已报，**仍在** |
| 15 | N-08 | **P2** | 文件内容进不了 `/api/search/smart` | 文档知识只能走语义直查 | 上轮已报，**仍在** |
| 16 | N-21 | **P2** | 崩溃续跑成功后 `extraction_error` 残留失败文案 | `status=completed` 但 error 说"进程在处理中途退出" | **新** |
| 17 | N-22 | **P2** | 文件跨 scope 重复入库（同 scope 才去重） | 同一份 md5 出现两条记录、两套向量 | **新** |
| 18 | N-23 | **P3** | 三份文档对同一行为给出不同数值 | 合并阈值 0.96/0.90 vs 0.75–0.90；端点 45/38 vs 42/35 | **新** |
| 19 | N-24 | **P3** | 响应示例与真实结构不符、错误码约定自相矛盾 | `extract_status` 示例没有 `success/data` 包裹；待办 404 违反"业务错误走 200" | **新** |
| 20 | N-25 | **P3** | 越界参数不拦：`event_limit=500`（文档称上限 100）、`export limit=999999` | 参数上限只写在文档里 | **新** |
| 21 | N-26 | **P3** | 人名里的数字被剥掉 | `liuda1999` 存成 `liuda` | **新** |

> 上一份的 N-03（merge 假成功）、N-10（文件嵌入拖慢抽取）、N-13（画像关系标签错误）本轮**未复测**，不改变结论，仍在待修列表里。
> 上一份的 X-10/X-12/X-13/X-14/X-19/X-20/X-21/X-22 同样未复测，结论沿用。

---

## 2. 本轮新发现（逐条）

### N-14 【P1】日期容错不一致：带空格、斜杠、中文数字全落"今天+7"

**现象（A/B 实测，同一句话只差写法，今天 = 2026-11-01）**

| 输入 | 期望 | 实际 |
|---|---|---|
| `提醒我11月12号前把索引重建脚本交出来` | 11-12 | **11-12** ✅ |
| `提醒我 11 月 12 号前把索引重建脚本交出来` | 11-12 | **11-08**（今天+7）❌ |
| `提醒我11/12前把索引重建脚本交出来` | 11-12 | **11-08** ❌ |

这不是孤例。本轮 15 条真实记录里，"提醒我 11 月 8 号前把合并后的测试报告交给用户"落库也是 11-08——**恰好等于今天+7**，无法排除走了同一条兜底。这种巧合比直接错更值得警惕：测试者和用户都会被"看起来对"骗过去。

**根因（精确到行）**

同一个文件里两条日期正则宽严不一：

```rust
// src/utils/time_parser.rs:220  —— 解析器，容错好
static_regex!(re_md, r"(\d{1,2})\s*[月\-/]\s*(\d{1,2})\s*[日号]?");

// src/utils/time_parser.rs:851 / 877 —— 候选抽取，容错差
r"...|\d{1,2}月\d{1,2}[日号]|\d{1,2}号)"
```

候选抽取用的那条**没有 `\s*`、不支持 `/`**。于是 `find_time_exprs_with_pos` / `find_precise_time_expr` 在 "11 月 12 号" 里找不到任何时间表达 → 返回 `None` → 上层落到 `default_deadline_in_days(7)`。解析器本身是能认的，**它根本没被调到**。

中文数字（上轮 N-02）是同一处的另一面：`\d` 不认"十二"。

**影响**

截止日期错 4–40 天不等，且**没有任何日志或字段提示"这是猜的"`。对智能体来说，记住"11 月 12 号"却报出 11-08，比不记更糟——它会据此提前或延后行动。

**修法**

1. 三条正则统一到一处（把 220 那版提成公共常量），加上 `[一二三四五六七八九十]+` 的中文数字分支；
2. 抽取不到时间表达时，**不要静默 +7**：`deadline` 留空，另存 `deadline_source: "none"`；确实要给兜底就把来源写进字段（`"guessed_plus_7d"`）；
3. 回归用例至少覆盖：`11月12号` / `11 月 12 号` / `11/12` / `十二月二十号` / `11月12日`。

---

### N-17【P1】"本周X"允许指向过去，待办创建即失效

**现象**

2026-11-01（周日）我说："我答应用户这周五给出一份按级别排序的问题清单。"
落库：`deadline = 2026-10-30`（已过去 2 天），`status = auto_invalid`——**创建当天就被判失效，用户在待办列表里永远看不到它**。

**根因**

```rust
// src/utils/time_parser.rs:483
let this_week = text.contains("本周") || text.contains("这周") || text.contains("本星期");
// src/utils/time_parser.rs:502
} else if this_week {
    // 本周：允许指向已经过去的那一天
    target_offset - days_since_monday
}
```

注释写得很明白，是**有意**允许过去的。设计的出发点是"补记昨天发生的事"，但同一个解析器也被待办截止时间复用，于是"本周五"在周日就变成了过去时。

**影响**

与 N-14 同类但更隐蔽：日期不是错的，是"合法地过去了"。结果是一条真实待办在用户看到之前就消失，而且 `auto_invalid` 状态没有任何入口能让用户发现。

**修法**

- 待办场景（`resolve_deadline_in_text`）里，"本周X"若解出过去日期 → **顺延到下周同一星期几**，或直接判为"无法唯一确定"而不给日期；
- `auto_invalid` 的待办应在 `GET /api/todos` 里有独立分组或字段提示，不能和正常待办混在一起静默存在。

---

### N-15【P1】事件 `summary` 落的是标题副本，`event_type` 落的是描述

**现象（库里直接看）**

```
title   : 第三轮测试用 2MB 无空行文本压文件链路
summary : 第三轮测试用 2MB 无空行文本压文件链路        ← 和 title 一模一样
event_type: 第三轮测试用 2MB 无空行文本压文件链路，切成 1758 块，入库耗时 10 分 26 秒…   ← 这是描述
```

**根因**

```rust
// src/service/background_queue.rs:626
let event_desc = e.title.as_deref().unwrap_or(&e.description);   // title 有值就只用 title
let dedup_key: String = event_desc.chars().take(200).collect();

// src/service/background_queue.rs:845 / 847
summary: dedup_key.clone(),      // 于是 summary = 标题
event_type: llm_description,     // 于是 event_type = 描述
```

同步路径（`three_library_manager.rs:579`）反而是对的：`summary: e.description`、`event_type: String::new()`。**两条写入路径语义不一致**，而真实流量走的是前者。

**影响**

1. 任何读 `summary` 的地方（联系人详情的 events、检索结果展示）只能看到被截断的标题；
2. `event_type` 这一列永远不可能是类型，所以 N-05 的"按类型过滤无效"不是查询写错了，是**数据层就没有类型**；
3. 去重键退化为"标题"，同一标题不同内容的事件会被静默丢弃。

**修法**

`summary` 用 `description`，`event_type` 留空或按 `importance` 映射；`dedup_key` 用 `title + description`（同步路径已是这么写的，把异步路径对齐即可）。

---

### N-18【P1】语义兜底无阈值、无置信度

**现象**

| 查询 | 返回 |
|---|---|
| `跨命名空间访问返回什么` | history=5（首条是"跳过 X-01 越权"那段，语义无关） |
| `火星殖民计划的时间表` | history=2 |
| `量子计算机采购预算` | history=3 |

完全不存在的话题也照样返回记忆，且结果里**没有 `similarity`、没有 `source`**：

```json
{"content":"用户明确要求跳过 X-01 越权那条…","created_at":"...","id":"..."}
```

**根因**

`/api/search/smart` 的语义兜底在"关键词命中 < 2"时无条件补位，没有最低相似度门槛；回源字典库后只取了 `content/timestamp`，把向量分数丢了。

**影响**

对智能体是**最危险的一类错误**：它分不清"确实没有相关记忆"和"返回了一堆不相关的"。文档明确承诺"全空结果时可据此判断是确实没有记忆"，现在这个判断依据不存在了。

**修法**

1. 兜底加阈值（e5-base 中文无关句基线约 0.84–0.89，建议 ≥0.90 才回）；
2. 每条兜底结果带 `source: "semantic"` + `similarity`；
3. 一个都没过阈值就返回空，并给 `matched_terms`，让调用方知道是"问法没被识别"。

---

### N-19【P2】待办 `contact_id` 过滤是死参数

**现象**：`GET /api/todos?user_id=agent_me&contact_id=not-a-real-id` 返回 1 条（该待办 `contact_ids` 为空）。

**根因**：路由层把过滤条件塞进去了，运行时没被读。

```rust
// src/api/routes.rs:2415-2419  —— 塞进 filter
let filter = if let Some(ref contact_id) = req.contact_id {
    filter.with_contact(contact_id.clone())
} ...

// src/api/async_memory.rs:548-551  —— 只取三样，contact_id 丢弃
todo_store.get_all_todos(owner_type, &owner_id,
    filter.status.as_deref(), filter.limit, filter.include_archived).await
```

API.md 明确写"`GET /api/todos` 支持 `status` / `contact_id` / `include_archived` 过滤"。

**修法**：`get_all_todos` 增加 `contact_id` 形参并在 SQL 里按 `contact_ids LIKE` 或关联表过滤；或用 `#[deny]` 级 lint 把"构造了但没消费的 filter"在编译期挡掉。

---

### N-20【P2】"知识"维度没有实现

README 开篇："守心在后台拆成**人物、事件、待办、知识**，写进三套存储"。
ARCHITECTURE §2 数据流也写着"结构化解析（人物 / 事件 / 待办 / **知识**）"。

实际情况：

- 21 张表里**没有任何知识 / learning / fact 表**（`ambiguity_log / contact_* / entity_alias / event_* / file_* / memory_processing_state / pending_event_merge / slice_* / todo_list / virtual_*`）；
- 抽取提示词（`extractor.rs:296` 起的"提取规则"段只列了 people / events / todos / todo_update_intent）**没有 facts、没有 learnings**；
- 模型实际返回确实是 `{"people":[],"events":[],"todos":[],"todo_update_intent":null}`；
- `POST /api/memory/add` 响应里的 `learnings` 字段恒为 `[]`。

`RawFact` / `RawLearning` 结构体和 `writer.write_learnings` 都存在，但**只有正则降级路径会往里塞**，正常 LLM 路径永远空。

**影响**：文档宣称的四维抽取实际只有三维；"知识"没有任何落库与检索入口。属于"宣称了但没做"，建议要么补提示词 + 建表，要么在 README 里删掉这个词。

---

### N-16【P2】同一件事生成两条待办

```
把 internal_api_token 默认空串改成必填   | due 2026-11-08 | created 20:34:56
将 internal_api_token 的默认空串改成必填 | due 2026-11-08 | created 20:40:30
```

同一句话（"开源前的待办：把 internal_api_token 的默认空串改成必填…"）产出两条措辞微差的待办。`todo_list` 没有 `source_conversation_id` 列，事后无法追是哪条对话产生的——这本身也是个可观测性缺口。

**修法**：写入前按"标题子序列匹配 + 同 owner"查重（`update_todo_deadline` 已有子序列匹配实现，可直接复用）；`todo_list` 补 `source_conversation_id`。

---

### N-21【P2】续跑成功后 `extraction_error` 残留失败文案

**现象**（强杀 → 重启）：

```
[5s]  status=processing  error=进程在处理中途退出，状态已回退为 failed（原文保留，可重放）
[20s] status=completed   error=进程在处理中途退出，状态已回退为 failed（原文保留，可重放）
```

状态机本身是对的（原文保留、自动重跑、20 秒完成，值得肯定），但**成功之后错误信息没清**。

**影响**：任何按 `extraction_error != null` 判断"这条坏了"的监控或智能体，会把成功的记录误判为失败。

**修法**：进入 `completed` 时置 `extraction_error = None`；把"上次失败原因"另存 `last_error` 字段。

---

### N-22【P2】文件跨 scope 重复入库

同一份 `docs/API.md`（md5 `f4e827e2…`）：

- `scope=user` 传一次 → 新记录
- 不带 scope（默认 agent）再传 → **`is_duplicate=false`，又建一条**
- `scope=user` 再传一次 → `is_duplicate=true`，返回原 `file_id`（同 scope 去重是好的）

结果库里两条 `file_index`、两套向量。叠加 N-06，其中一套永远不可见——**纯占空间**。

---

### N-23 / N-24 / N-25 / N-26（P3，文档与体验）

| 编号 | 问题 | 证据 |
|---|---|---|
| N-23 | **三份文档对同一行为给出不同数值** | 合并阈值：`config.toml` 与 CONFIGURATION.md = `0.96/0.90`；API.md「事件合并确认」与 ARCHITECTURE §4 = `0.75–0.90`。端点计数：README = "45 注册 / 38 唯一"，ARCHITECTURE §10.1 = "42 / 35"。冷表可见性：API.md/ARCHITECTURE = "默认包含归档"，CONFIGURATION §8 = "默认查询只看热表"。实测代码是 45/38、默认包含归档，即**后两份文档错** |
| N-24 | **响应示例与真实结构不符** | `extract_status` 文档示例是扁平的 `{"conversation_id","status"}`，实际是 `{"success":true,"data":{…}}`；`pending_merges` 示例是 `{"count","merges"}`，实际是 `{"success":true,"data":{"count","merges"}}`；`GET /api/todos/:id` 不存在返回 **404 + 英文 `Todo not found`**，与"业务错误走 200 + 中文 error"的约定冲突 |
| N-25 | **参数上限只写在文档里** | `event_limit=500`（文档称上限 100）被接受；`export limit=999999` 被接受 |
| N-26 | **人名数字被剥** | `liuda1999` → `liuda`（`extractor.rs:668` 的 `take_while(!is_ascii_digit)`）。对真人名去尾号有用，对账号/花名是破坏 |

---

## 3. 上一轮问题复验状态

| 上轮编号 | 问题 | 本轮结果 |
|---|---|---|
| N-01 | 历史导入时间基准丢失 | ❌ **仍在**，且拿到更硬的证据（10-20 说的"下周三"→11-04，应为 10-28） |
| N-02 | 中文数字日期不解析 | ❌ **仍在**，并入 N-14（同一条候选正则） |
| N-03 | merge/create 只改状态 | 未复测（0.96 阈值下待确认队列为空，造不出样本） |
| N-04 | history 的 limit 兼作搜索深度 | ❌ **仍在**：`query=报告` `limit=2`→0，`limit=5`→1 |
| N-05 | 事件三字段被忽略 | ❌ **仍在**，且**根因升级**为 N-15（数据层就没有类型） |
| N-06 | 文件默认 scope 向量不可见 | ❌ **仍在**，用真实文档复现：默认 scope 上传的 API.md 内容一条都搜不到 |
| N-07 | 中文整句关键词 | ❌ **仍在**，根因定位到 `routes.rs:2045 split_query_terms` |
| N-08 | 文件进不了 smart | ❌ **仍在**：问"跨命名空间访问返回什么"（README 原句），smart 只回对话，不回文档块 |
| N-09 | 虚拟角色断裂 | ❌ **仍在**：`check/:name` 能查到，`search/contacts?include_virtual=true` 返回 0，`/api/contacts/:id/detail` 404 |
| N-10 | 文件嵌入拖慢抽取 | 未复测 |
| N-11 | 导出三张表 / 死配置 | ❌ **仍在**：`counts={contacts,events,todos}`，无原文 |
| N-12 | 兜底结果缺 similarity/source | ❌ **仍在**，且升级为 N-18（还多了"无阈值"） |
| N-13 | 画像质量 | 未复测 |

**上一轮修好的 6 项，本轮全部保持修好状态**：X-02（待办 query 认 user_id）、X-06（分块 1200 字）、X-07（失败落状态 + 清临时文件）、X-08（contact_id 可解析）、X-11（超时接配置）、X-04 的 query/倒序部分。

---

## 4. 文档符合性矩阵（本轮实跑）

✅ = 与文档一致；⚠️ = 部分；❌ = 不一致

| 文档承诺 | 结果 | 说明 |
|---|---|---|
| `POST /api/memory/batch` ≤500 条、逐条返回 | ✅ | 15 条接收 **18.8ms**；501 条返回 `400 单批最多 500 条，当前 501` |
| `timestamp` 三种写法（RFC3339 / 本地 / 仅日期） | ✅ | 仅日期按当天 12:00 归档已实测 |
| `timestamp` 非法 / 未来 >1 天 / 早于 2000 → 400 | ✅ | 三条都返回 400，错误信息清晰 |
| `messages` 保留"我：/对方：" | ✅ | 落库原文为 `我：报价单…\n对方：我周五前发给你。` |
| 会话日按 `day_cutoff_hour=4` 切分 | ✅ | **03:59→10-30、04:00→10-31、仅日期→12:00**，三点全中 |
| `dictionary/days` 倒序 + `before` 翻页 | ✅ | `[('2026-11-01',7),('2026-10-31',5),…]` |
| `dictionary/day/:date` 升序 + `offset/has_more` | ✅ | |
| `dictionary/stats` 自检一致性 | ✅ | `conversations=25, days=5` |
| `dictionary/conversation/:id` 跨 owner → 404 | ✅ | 越权读他人原文返回 `conversation not found`，不泄露存在性 |
| `POST /api/todos?user_id=` 认 query | ✅ | `owner_id=agent_me`，未落 `default_user` |
| 待办无字段级更新 → 405 | ✅ | `PUT /api/todos/:id` 返回 405 |
| 待办 `status` 过滤 | ✅ | |
| 待办 `contact_id` 过滤 | ❌ | 见 N-19 |
| `POST /api/backup` 生成 VACUUM 快照 | ✅ | 0.3 MB，文件可打开 |
| `GET /api/export` 含 counts/contacts/events/todos | ⚠️ | 字段齐，但不含对话原文（N-11） |
| 虚拟角色 CRUD + `check/:name` + 同名不同作品共存 | ✅ | 列表 12 条（内置 + 本轮自建），`(name, source_work)` 唯一键生效 |
| 虚拟角色"共用联系人检索/详情" | ❌ | 见 N-09 |
| `POST /api/vector/search` 直查 | ✅ | 返回 material + score |
| `replay` / `replay_failed` | ✅ | `replayed:true, status:pending`；不存在的 id 返回 400 |
| `heartbeat/status` `/pending_questions` `/trigger` | ✅ | `maintenance_only` 秒级返回 |
| 事件默认包含归档 | ✅（代码） | 本轮无归档事件，按代码与上轮 120 天实验确认 |
| 事件 `query` / `hot_only` / `archived_month` | ❌ | 用不存在的词查 `query` 仍返回 5 条 |
| 历史检索 `query`/`limit`/倒序 | ⚠️ | 倒序 ✅、limit 生效 ✅，但 limit 兼作深度（N-04），且"分词"名不副实（N-07） |
| 融合检索 `matched_terms` | ⚠️ | 字段有，但返回的是**整句话** |
| 语义兜底带 `source` + `similarity` | ❌ | 见 N-18 |
| 崩溃后队列续跑 | ✅ | 强杀后重启，20 秒内完成，原文未丢 |
| 模型未就绪时 `/health` 语义 | ⚠️ | 上轮实测 `/health` 早于模型就绪即返回 200（本轮进程 8 秒就绪，无法区分） |
| CPU 预算 60% | ✅（上轮） | 上轮实测均值 60.4%、P95 65.8% |

---

## 5. 值得肯定的部分

不是只来挑刺的。这一轮真实工作负载跑下来，以下能力是**扎实可用**的：

1. **写入链路 18.8ms 收 15 条**，`conversation_id` 立即返回，不阻塞调用方——"写入不阻塞智能体"这条设计价值观落地了。
2. **会话日切分做得很讲究**：04:00 边界、只给日期按 12:00、`session_day` 固化后再重算比对，三点全对。这是本轮最漂亮的一块。
3. **崩溃续跑真的能用**：强杀在抽取中途，重启后自动认出"处理中途退出"，回退状态、重跑、20 秒完成，原文一字不丢。
4. **跨租户隔离没有裂缝**：拿别人的 `conversation_id` 直查返回 404，不泄露存在性。
5. **语义检索质量够用**：10 个真实工作问题、问法不含原句关键词，**9/10 命中**。
6. **批量上限、时间戳校验、405、405 语义**这些"容易糊弄"的边界都认真做了，错误信息还是中文的具体原因。
7. **文件入库**：真实 21KB 的 API.md 切成块后能被检索到（`file_chunk` 命中 0.86），说明"上传→切块→嵌入→检索"这条链是通的。

---

## 6. 如果只能修三件

| 顺序 | 修什么 | 为什么是它 |
|---|---|---|
| **1** | **N-01 + N-14 + N-17（时间语义三件套）** | 三条同源，都是"日期错了还不吭声"。记忆系统里时间是主键之一，错了等于记忆全废。而且改动集中在 `time_parser` 一个文件，性价比最高 |
| **2** | **N-18 + N-07（检索可信度）** | 一个让智能体拿到无关记忆，一个让智能体找不到相关记忆。两者叠加，检索层就没有可信度可言了 |
| **3** | **N-15 + N-05（事件字段与参数）** | 改两行赋值（`summary`/`event_type`），顺带把"按类型过滤"这条链路打通 |

后面依次：N-06/N-22（文件 scope 与去重）→ N-19（待办过滤）→ N-20（知识维度：要么做，要么从文档删掉）→ P3 一批文档订正。

---

## 7. 复现方法

```bash
# 1. 干净实例
rm -rf data_a2 && ./target/release/shouxin-server.exe --data-dir ./data_a2

# 2. 批量导入真实工作记忆（15 条，含真实时间戳）
python3 E:/tmp/a2/seed.py

# 3. 文档符合性（含错误约定、会话日、待办、备份、虚拟角色）
python3 E:/tmp/a2/conform1.py
python3 E:/tmp/a2/conform1b.py

# 4. 检索参数与边界
python3 E:/tmp/a2/conform2.py

# 5. 三路召回（10 个真实问题）
python3 E:/tmp/a2/recall.py

# 6. 日期 A/B：空格 vs 无空格
curl -X POST localhost:3000/api/memory/add -H 'Content-Type: application/json' \
  -d '{"conversation":"提醒我11月12号前交脚本","user_id":"probe_nospace"}'
curl -X POST localhost:3000/api/memory/add -H 'Content-Type: application/json' \
  -d '{"conversation":"提醒我 11 月 12 号前交脚本","user_id":"probe_space"}'

# 7. 崩溃续跑：写入后立刻强杀，再启动
curl -X POST localhost:3000/api/memory/add -H 'Content-Type: application/json' \
  -d '{"conversation":"崩溃续跑探针","user_id":"probe_resume"}'
taskkill //F //IM shouxin-server.exe
./target/release/shouxin-server.exe --data-dir ./data_a2
curl localhost:3000/api/memory/extract_status/<conversation_id>
```

测试脚本全部留在 `E:/tmp/a2/`，数据在 `data_a2/`（约 12 MB），未触碰 `data/` 及历史 `data_probe*`、`data_v070`。

---

*报告结束。共 21 条问题：P1 8 条、P2 9 条、P3 4 条。其中本轮新增 13 条（上轮的 N-02 并入 N-14、N-12 升级为 N-18），复验仍在 8 条，未复测 3 条。*
