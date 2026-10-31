# 守心 v0.7.3 基础功能复测 + “修复质量”专项审计

> **修复请直接看精简版**：`docs/FIX_P0_P1_v0.7.3.md`（只含 P0/P1，逐条给根因、改法、验收）。
> 本文保留完整证据（P0–P3、审计表、复现命令），供复核。

- 测试时间：2026-11-01 05:45–06:05（本地）
- 被测版本：`shouxin 0.7.3`，commit `1cf57a2`，release 二进制（构建 05:43）
- 运行方式：独立配置 `E:/tmp/v073/config.toml`，端口 `3103`，数据目录 `E:/tmp/v073/data`（**未触碰仓库内 `data/`、`data_*`**）
- 模型：`gemma-4-E4B-it-Q4_K_M`，纯 CPU，16 物理核
- 语料：全新编写 13 段花店经营对话（9 段 + 4 段），5 个真人联系人，1 个虚拟角色，1 份文档
- 官方冒烟：`PASS=39 FAIL=0 SKIP=1`（可复现）
- 单元测试：`cargo test --lib` → **105 passed / 0 failed**（文档写 109 / 113，见 P3-1）

---

## 0. 结论先说

**不是“全盘造假”，但确实是“按报告样例逐条打补丁”。**

- 真实修好的：`history limit`、文件默认 `scope`、文件进融合检索、虚拟角色镜像、jieba 分词、无关查询阈值、`contact_id` 过滤、事件 `limit` 上限、备份不覆盖、`event_type` 显式 400。
- 但修复普遍**只覆盖报告里出现的那一句话/那一个入口**，没有沿数据流验证。换一种说法、换一个入口、换一个月，就会失败。
- 本次抽测 20 条修复声明：**11 条成立、4 条只部分成立、4 条不成立、1 条未测**。
- 其中 4 条不成立直接造成数据错误或回归：
  1. 模型日期仍覆盖本地解析，截止时间可以错 14 个月（P1-1）；
  2. 事件写路径改了、读路径没改，事件检索 `description` 恒为空（P1-2）；
  3. 姓名清洗硬编码列表把“王建国”截成“王建”，真名检索不到（P1-3）；
  4. `v0.7.3` 声称修好的“不存在 merge_id”仍返回 `resolved:true`——改的是另一个端点（P1-4）。

因此 README/CHANGELOG 里“8 条 P1 全修”“113/113”“15 项全过”的验收结论，**在本机复现不成立**。

---

## 1. 基础功能跑通情况

### 1.1 通过项

| 环节        | 结果 | 证据                                                                |
| --------- | -- | ----------------------------------------------------------------- |
| 启动 / 健康检查 | 通过 | `/health` 200，0 ERROR / 0 PANIC                                   |
| 批量写入      | 通过 | 13 条全部 accepted，18ms                                              |
| 后台抽取      | 通过 | 13/13 `completed`，0 `failed`                                      |
| 重启持久化     | 通过 | 重启后 12 条待办、3 条事件、虚拟角色均还在；`recovered 0 pending tasks`              |
| 官方 API 冒烟 | 通过 | `PASS=39 FAIL=0 SKIP=1`，与仓库声明一致                                   |
| 联系人检索     | 通过 | 何晓岚 / 叶知秋 / 老陈 / 沈望舒 / 张罗锅 均可命中                                   |
| 历史检索      | 通过 | `query=芍药&limit=2` 返回 1 条（旧版会返回 0）                                |
| 融合检索      | 通过 | 中文问句分词为 `["中秋","采购","账表"]`，能命中待办                                  |
| 无关问题      | 通过 | “南极磷虾捕捞配额” → events/history/todos 全 0                             |
| 文件链路      | 通过 | 不带 `scope` 上传可读；`files` 命中 `v073-note.md`，similarity 0.891        |
| 虚拟角色      | 通过 | 创建 → 详情 200 `is_virtual=true` → `include_virtual=true` 可搜 → 默认不混入 |
| 备份 / 导出   | 通过 | 重复备份 400；导出 `limit=10001` 400；正常导出 counts 正确                      |
| 待办错误语义    | 通过 | 不存在的待办返回 200 + `success:false`                                    |
| 参数上限      | 通过 | 事件 `limit=201` → 400；`event_type` 非空 → 400                        |

### 1.2 未通过 / 异常项（详见第 2 节）

| 环节     | 结果 | 现象                                        |
| ------ | -- | ----------------------------------------- |
| 事件检索内容 | 失败 | `description` 恒为空（P1-2）                   |
| 待办截止时间 | 失败 | 多条日期错周 / 错年 / 错到当天（P1-1、P1-5）             |
| 姓名抽取   | 失败 | “王建国”存成“王建”（P1-3）                         |
| 合并确认   | 失败 | 不存在的 id 仍 `resolved:true`（P1-4）           |
| 待办查重   | 失败 | 两条“中秋对账表”同时 pending（P2-1）                 |
| 事件抽取   | 部分 | 13 段对话只有 2 段产出事件；过去事件被拆成 2 条待办（P2-3）      |
| 融合检索去重 | 部分 | 同一事件同时以结构化 + 语义两条返回（P2-2）                 |
| 心跳     | 部分 | 自动写入 title/event_time 均为 NULL 的碎片事件（P2-4） |



---

## 2. 问题清单（按严重度）

### P1-1 截止时间仍以模型日期为准，对话时间只影响“兜底”

**结论：`N-01` 只修了 fallback，没有修主路径。**

`background_queue` 现在确实把 `conversation_time` 传给了本地解析器，但代码先看模型给的 `deadline`：

```rust
// src/service/background_queue.rs:1018-1021
let model_ok = model_deadline.filter(|dt| {
    *dt >= ref_day_start && *dt < ref_time + chrono::Duration::days(365 * 5)
});
```

只要模型日期 ≥ 对话当天，就采信；而抽取提示词里**没有任何“对话发生时间”**（`extractor.rs:290-350` 是静态 prompt），模型只能猜。结果：

| 对话时间       | 原话                       | 期望         | 实际落库                          | 来源             |
| ---------- | ------------------------ | ---------- | ----------------------------- | -------------- |
| 2026-08-03 | 下周三是……最后期限               | 08-12      | **08-03**（当天，立即 auto_invalid） | 本地解析取错（见 P1-5） |
| 2026-09-21 | 11 月 12 号前把 2027 年价目表定下来 | 2026-11-12 | **2027-11-12**                | 模型             |
| 2026-06-10 | 下周三之前把排班表发出去             | 06-17      | **06-10**                     | 模型             |
| 2026-06-10 | 下下周五之前把检修单送过来            | 06-26      | **07-22**                     | 模型             |
| 2026-06-10 | 下周三前把报价单发出去              | 06-17      | **06-18**                     | 模型             |
| 2026-06-10 | 6 月 24 号之前把花材清单定下来       | 06-24      | 06-24（正确）                     | 模型             |

6 月 4 条全部走了模型日期；只有“6 月 24 号”这种原文写死日期的碰巧对。模型日期还统一是 `00:00:00Z`，与本地解析的 `23:59:59` 口径不一致。

**修复建议**

1. prompt 里注入 `对话发生时间：{conversation_time}`，并明确“相对时间以该时间为基准”；
2. 模型日期与本地 `resolve_deadline_in_text` 结果交叉校验：本地有解且相差 > 1 天时以本地为准；
3. 上限从“5 年”收紧到“对话后 2 年”，并对“对话当天”单独告警（大概率是解析失败）。

---

### P1-2 事件 `description` 恒为空：写路径改了，读路径没改

`N-15` 把 `event_type` 列清空、把描述存进 `summary`：

```rust
// src/service/background_queue.rs:861-863
summary: e.description.chars().take(200).collect(),
event_type: String::new(),
```

但事件检索仍把 `event_type` 当描述返回：

```rust
// src/memory/three_library_manager.rs:1241
description: trait_event.event_type.clone(),
```

实测：

```json
POST /api/memory/search/events {"query":"报关"}
{ "title": "报关单被扣", "description": "", "time_info": "2026-12-17T00:00:00+00:00" }
```

数据库里 `summary` 明明有完整描述；`/api/contacts/:id/detail` 也能返回 `summary`。只有主检索接口和 `/api/search/smart` 的结构化事件是空描述——**凡是走 HTTP 异步链路写入的事件，description 都为空**。

**影响**：调用方只能看到标题，看不到事件内容；智能融合检索的结构化结果同样残废。  
**修复**：`description` 改取 `summary`（并保留 `summary` 字段），或给 `EventDetail` 增加 `summary`。  
**同类**：`related_contacts: vec![]` 也是硬编码（`three_library_manager.rs:1245`），DB 里 `contact_ids` 有老陈，接口却永远为空。

---

### P1-3 姓名清洗硬编码把“王建国”截成“王建”

官方冒烟写入的对话：

> 老王是我的同事，在研发部做后端，本名**王建国**。小李和老王是同一个项目组。下周五之前要交付接口文档。

日志：

```
[Name Fix] Truncated name detected: '王建国' -> '王建'
```

落库联系人 `name=王建`，`aliases=["老王"]`，**联系人记录的任何字段里都不再有“王建国”**（原文只留在字典库对话里）。实测：

- `search/contacts {"query":"王建国"}` → 0 条
- `search/contacts {"query":"王建"}` → 1 条

根因是 `fix_name_truncation` 的后缀表里包含单字“国”：

```rust
// src/memory_extractor/extractor.rs:645
"国", "家", "城", "市", "县", "省", "区", "镇", "村",
```

任何 3 字以上、以“国”结尾的中文名都会被砍（建国、卫国、志国、兴国……）。`N-26` 只修了“数字后缀”，相邻的这条规则没动。

**修复建议**：删除单字地理/常用字后缀；只保留“的/了/是/在/和/说”这类明显非姓名尾字；对 2–4 字中文名一律不截断；补一条 `王建国/李卫国/张兴国` 的回归测试。

---

### P1-4 `v0.7.3` 声称修好的 merge 错误语义，修在了另一个端点

文档中的端点：

```http
POST /api/events/pending_merges/999999/resolve  {"action":"merge"}
```

实测仍返回：

```json
{"success": true, "data": {"action":"merge","merge_id":999999,"resolved":true}}
```

根因：`resolve_pending_event_merge` 只执行 UPDATE，不检查 `rows_affected`，永远 `Ok(())`：

```rust
// src/stores/relational/store.rs:3159-3172
UPDATE pending_event_merge SET status='resolved' ... WHERE id = ?
...
Ok(())
```

`v0.7.3` 改的是 `confirm_pending_event_merge`（`store.rs:3495`，Endpoint `/api/events/confirm_merge`）：

```json
POST /api/events/confirm_merge {"merge_id":999999,"action":"merge"}
{"success": false, "error": "实体未找到: 待确认合并记录不存在或已处理"}
```

**也就是说：改对了一个，但用户按文档调用的是另一个。** 冒烟脚本只用“存在的 id”测 resolve，所以 39/39 完全发现不了。

**修复**：`resolve_pending_event_merge` 检查 `rows_affected()==0` 返回 `EntityNotFound`；或把两个端点合并成一个。文档同步。

---

### P1-5 两种常见句式仍算错截止日

**(a) “X 是……最后期限”**：截止标记只认“之前/以前/前/截止”，不认“最后期限/期限/到期”。识别不到标记后，退回“第一个能解析的时间”，而句首有“今天上午”：

> 今天上午和何晓岚盘完账……**下周三是**供应商老陈承诺补货的**最后期限**

→ 取到“今天上午” → `2026-08-03`，待办创建即 `auto_invalid`，用户永远看不到。  
代码：`src/utils/time_parser.rs:923-943`。

**(b) “下下周五”少一周**：扫描器能匹配“下下周五”，但解析器只判断 `contains("下周")`：

```rust
// src/utils/time_parser.rs:580
let next_week = text.contains("下周") || text.contains("下星期");
```

“下下周五”包含“下周”，于是按“下周五”算。2026-09-08（周二）说“下下周五之前” → 落库 **09-18**（应为 09-25），早一周。

**修复建议**

- 截止标记补 `最后期限/期限/到期/截止日期`；仍无标记时，优先取**句末最近**的时间表达，而不是第一个；
- `parse_week` 显式处理 `下下周`（+14 天）与 `上上周`，不要用 `contains("下周")`。

---

### P2-1 待办查重只覆盖“把/将”这一种改写

同一件事的两段对话：

- “我答应何晓岚本周六之前把中秋节的采购**对账表**弄完。”
- “再记一下，中秋采购**对账表**要在本周六前做完，别拖。”

落库两条 pending：

```
完成中秋节采购对账表   2026-11-07
中秋采购对账表         2026-11-07
```

字符集合 Jaccard = 7/10 = **0.70** < 0.85；`contains` 也因为中间多一个“节”而不成立。

`todo_titles_similar` 的测试用例恰好是“把…改成 / 将…的…改成”（只差一个虚词，Jaccard 0.93），所以单测过、真实改写不过。这是典型的“按样例调参”。

**修复建议**：先剥离“完成/处理/进行/记得/再记一下”等动词前缀，再算 token 级相似度（或编辑距离/最长公共子序列），阈值不要只靠字符集合。

---

### P2-2 融合检索同一事件返回两条

`/api/search/smart {"query":"玫瑰什么时候到港"}` 返回：

```json
[
  {"event_id":"cea2…","title":"报关单被扣","description":""},
  {"title":"事件: 报关单被扣 - 厄瓜多尔玫瑰…","similarity":0.911,"source":"semantic"}
]
```

关键词通道命中 1 条，语义兜底又补 1 条，**没有按 event_id / 内容去重**；语义条目的 title 还是 `"事件: 标题 - 描述"` 的拼接串。`B4` 同样重复。

**修复**：兜底前收集已有 event_id/内容指纹，命中则跳过；语义条目的 title 取 `material.summary`，不要拼 content。

---

### P2-3 事件抽取召回低，且“过去事件”被当成待办

13 段对话只有 2 段产出事件（报关被扣、橱窗方案）；`Fallback event extraction: 0 events` 出现 7 次，正则兜底完全没兜住。

最典型的一段：

> 盘点完发现冷柜温度记录仪坏了三天，冻伤了两箱绣球，损失大概四千二。

模型输出 `events: []`，改为两条待办：“检查并修复温度记录仪”“处理冻伤的绣球”，截止日都按 +7 天猜。这是已经发生的事件，不是待办。

**影响**：事件记忆大量缺失，且错误地污染待办列表。  
**建议**：提示词里增加“已发生的事 → events，未来要做 → todos”的对照示例；正则兜底至少识别“发现/坏了/损失/被扣/到货”等事件词；对“坏了三天”这类完成态句子禁止生成待办。

---

### P2-4 心跳会写入无标题、无时间的事件碎片

心跳把人物画像里的 `major_events` 直接写成 `EventIndex`：

```rust
// src/heartbeat/analyzer.rs:652-666
summary: event.summary.clone(),
title: None,
event_time: None,
```

实测多出一条：

```
title=NULL, event_time=NULL, summary="预算上限三万八 预算上限三万八。"
```

并触发一条 0.903 的待确认合并。它既不能按标题检索，也无法按时间排序，只会制造噪声。

**建议**：`major_events` 只更新画像，不直接建事件；要建就要求有 title + 时间，或从来源对话回填。

---

### P3-1 验收数字对不上

- `cargo test --lib` 实际 **105 passed**；
- README 徽章/正文写 **109 项**（`README.md:12,456`）；
- CHANGELOG 写 **113/113**（`CHANGELOG.md:173`）。

三处三个数字。即使算上 feature 差异，文档也没有注明构建条件，不能作为“已验收”的证据。

---

### P3-2 文档仍是“改一半”

| 文档                                      | 问题                                                                                              |
| --------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `docs/API.md:635`                       | 导出 `limit` 写“上限 50000”，代码 `MAX_EXPORT_LIMIT=10_000`（`routes.rs:1623`），10001 返回 400              |
| `docs/API.md:302`                       | `matched_terms` 示例仍是整句 `["李明最近有什么变化","李明"]`，与“jieba 分词、不再整句”的修复矛盾                               |
| `docs/API.md:415`                       | 虚拟联系人开头写“**不在 contact_index 里**”，后文又写“创建时写入 contact_index 镜像”，自相矛盾                              |
| `docs/ARCHITECTURE.md:129-131`          | 合并阈值仍写 0.75 / 0.90，实际配置 0.90 / 0.96；`N-23` 声称统一了口径，只改了 API.md/README                            |
| `docs/API.md:357`、`ARCHITECTURE.md:135` | 都把 `/pending_merges/:id/resolve` 当唯一出口，但它正是 P1-4 里假成功的端点；真正修好的 `/api/events/confirm_merge` 没进文档 |
| `docs/ARCHITECTURE.md:35`               | 仍把“知识”列为结构化维度；README 已承认未实现                                                                     |
| `README.md:456`                         | 单元测试 109，实际 105                                                                                 |

---

### P3-3 未来时间戳的对话“写入成功但历史检索不可见”

`timestamp` 没有未来校验；`/api/memory/search/history` 的默认 `end_time = Utc::now()`（`routes.rs:2183-2198`）。本次两条 11-01 09:30 / 10:05 的对话（测试时本地 05:45）抽取正常、待办正常，但历史检索永远查不到。建议写入时拒绝未来时间，或检索默认 `end_time` 放宽。

---

## 3. “修复质量”审计表


| # | 修复声明 | 判定 | 依据 |
|---|---|---|---|
| 1 | N-01 截止时间全部以对话时间为基准 | 失败 | 模型日期优先，prompt 无对话时间；6 月 4 条全错（P1-1） |
| 2 | N-14 日期写法统一容错 | 部分 | 空格/斜杠/中文数字已好；“下下周X”少一周（P1-5b） |
| 3 | N-17 本周X 不指向过去 | 通过 | 周日“本周六” → 11-07 |
| 4 | N-07 jieba 真分词 | 通过 | 问句切出 `["中秋","采购","账表"]`，不再是整句 |
| 5 | N-18 语义兜底阈值 | 通过 | 无关查询 0 条；改写问法仍能召回 |
| 6 | N-04 history limit 只控条数 | 通过 | `limit=2` + “芍药” → 1 条 |
| 7 | N-15 summary 存描述、event_type 留空 | 部分 | 写对了；读路径仍取 event_type，description 恒空（P1-2） |
| 8 | N-05 事件 query 生效 | 通过 | `query=报关` → 1 条 |
| 9 | N-25 limit 超限 400 | 通过 | 事件 201 / 导出 10001 均 400 |
| 10 | N-16 待办查重 | 失败 | “完成中秋…对账表”与“中秋采购对账表”并存（P2-1） |
| 11 | N-19 contact_id 过滤 | 通过 | 何晓岚过滤出 2 条，contact_ids 均含她 |
| 12 | N-06 文件默认 scope=user | 通过 | 不带 scope 上传可读、可搜 |
| 13 | N-08 文件块进融合检索 | 通过 | `files=[("v073-note.md",0.891)]` |
| 14 | N-09 虚拟角色接入联系人 | 通过 | 详情 200 / include_virtual 命中 / 默认排除 |
| 15 | N-23 合并阈值口径统一 | 部分 | API.md/README 改了，ARCHITECTURE 仍是 0.75/0.90 |
| 16 | v0.7.3 不存在 merge_id 不再假成功 | 失败 | 文档端点仍 `resolved:true`，改的是另一个端点（P1-4） |
| 17 | v0.7.3 备份不覆盖 | 通过 | 第二次备份 400 + 明确提示 |
| 18 | v0.7.3 lib 113/113 | 失败 | 实际 105 passed（P3-1） |
| 19 | N-26 姓名数字后缀 | 部分 | 数字场景大概率好了；相邻“国”字截断仍在（P1-3） |
| 20 | N-21 完成后清 extraction_error | 未测 | 静态看逻辑正确，未做崩溃续跑 |

**统计：11 通过 / 4 部分 / 4 失败 / 1 未测。**

---

## 4. 建议修复顺序

1. **P1-1 时间主路径**：prompt 注入对话时间 + 模型/本地交叉校验 + 收紧上限。这是“历史导入”能不能用的前提。
2. **P1-2 事件读路径**：`description` 改取 `summary`，`related_contacts` 从 `contact_ids` 解析。两处小改，收益最大。
3. **P1-3 姓名截断**：删掉单字后缀表，补 3 字常见名回归。
4. **P1-5 句式**：`最后期限` 标记 + `下下周` 显式 +14 天。
5. **P1-4 merge**：`rows_affected` 检查；两个端点合并；文档只留一个。
6. **P2-1 查重**：先剥动词前缀，再 token 级相似度；不要用字符集合 Jaccard 一把梭。
7. **P2-2 去重**：关键词与语义通道按 event_id 去重。
8. **P3 文档/数字**：统一 105、导出 10000、合并阈值 0.90/0.96、删除“知识”维度、修正 `matched_terms` 示例与虚拟联系人开头。

---

## 5. 附录

### A. 测试数据落库结果（owner=`yezhiqiu`）

| 项 | 数量 |
|---|---|
| 对话 | 13（全部 completed） |
| 待办 | 12（pending 6 / auto_invalid 6） |
| 事件 | 3（2 条来自对话，1 条心跳碎片） |
| 真人联系人 | 5 |
| 虚拟角色 | 1 |
| 待确认合并 | 1（“预算上限三万八” 0.903） |

### B. 关键复现命令

```bash
# 启动（独立目录，勿用 data/）
SHOUXIN_CONFIG=E:/tmp/v073/config.toml ./target/release/shouxin-server.exe

# 1) merge 假成功
curl -s -X POST http://127.0.0.1:3103/api/events/pending_merges/999999/resolve \
  -H 'Content-Type: application/json' -d '{"action":"merge"}'
# -> {"success":true,"data":{"resolved":true}}

# 2) 事件描述为空
curl -s -X POST http://127.0.0.1:3103/api/memory/search/events \
  -H 'Content-Type: application/json' \
  -d '{"query":"报关","limit":5,"scope":"user","user_id":"yezhiqiu"}'
# -> "description": ""

# 3) 姓名截断（官方冒烟语料）
curl -s -X POST http://127.0.0.1:3103/api/memory/search/contacts \
  -H 'Content-Type: application/json' \
  -d '{"query":"王建国","limit":5,"scope":"user","user_id":"smoke073"}'
# -> 0 条；“王建” -> 1 条
```

### C. 代码位置索引

| 问题 | 位置 |
|---|---|
| 模型日期优先 | `src/service/background_queue.rs:1018-1021` |
| prompt 无对话时间 | `src/memory_extractor/extractor.rs:290-350` |
| 事件描述取 event_type | `src/memory/three_library_manager.rs:1241` |
| 事件写 summary/event_type | `src/service/background_queue.rs:861-863` |
| merge 无 rows_affected | `src/stores/relational/store.rs:3159-3172` |
| merge 正确校验（另一端点） | `src/stores/relational/store.rs:3495` |
| “最后期限”取第一个时间 | `src/utils/time_parser.rs:923-943` |
| “下下周”按“下周”算 | `src/utils/time_parser.rs:580` |
| 姓名后缀表含“国” | `src/memory_extractor/extractor.rs:645` |
| 待办 Jaccard 0.85 | `src/service/background_queue.rs:1229-1245` |
| 心跳事件 title=None | `src/heartbeat/analyzer.rs:652-666` |
| 事件检索 related_contacts 硬编码 | `src/memory/three_library_manager.rs:1245` |

---

**测试数据位置**：`E:/tmp/v073/`（config、data、日志、脚本、checks.json），未写入仓库 `data/`；测试结束后服务已停止。
