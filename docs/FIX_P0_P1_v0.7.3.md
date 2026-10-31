# 守心 v0.7.3 · P0 / P1 修复清单

> 只含 P0/P1，共 5 条，按「现象 → 根因 → 改法 → 验收」排列，可直接照着改。
> P2/P3 与完整证据见 `docs/TEST_REPORT_v0.7.3_2026-11-01.md`。
> 环境：commit `1cf57a2`，端口 `3103`，数据 `E:/tmp/v073/data`（未触碰仓库 `data/`），模型 `gemma-4-E4B-it-Q4_K_M`。

## 0. 总览

| 编号 | 一句话 | 落点 | 改动量 |
|---|---|---|---|
| P1-1 | 截止时间以模型猜的日期为准，提示词里没有对话时间 | `background_queue.rs:1020` + `extractor.rs:290` | 中 |
| P1-2 | 描述写进 `summary`，查询层当空值丢弃，读接口取空 `event_type` | `store.rs:1232` | 小 |
| P1-3 | 姓名清洗表的单字后缀含「国」 | `extractor.rs:645` | 小 |
| P1-4 | 文档推荐的合并端点只改状态、不合并、不校验影响行数 | `store.rs:3149` + `routes.rs:1811` | 中 |
| P1-5 | 「X 是……最后期限」「下下周五」解析错误 | `time_parser.rs:580` / `:923` | 小 |

**P0：0 项。** 口径：服务不可用、进程崩溃、跨租户越权、不可恢复的数据丢失，本轮均未出现。
最接近 P0 的是 P1-3（写库时真名被改写）和 P1-4（确认合并却什么都没做）；前者原文仍在字典库、可重放恢复，后者有可用替代端点，故列 P1。

**顺序**：P1-2 → P1-3 → P1-4 → P1-5 → P1-1。前四项改动小、可当场验证；P1-1 要动提示词并重跑抽取，放最后统一回归。

---

## P1-1 截止时间以模型日期为准，提示词里没有对话时间

**现象**：模型日期只要"不早于对话当天"就采信；提示词是静态的，模型不知道对话发生在哪天，只能猜。

| 对话时间 | 原话 | 期望 | 实际 | 偏差 |
|---|---|---|---|---|
| 2026-06-10 | 下周三之前把端午排班表发出去 | 06-17 | **06-10** | 提前一周，且是当天 |
| 2026-06-10 | 下周三前把报价单发给沈望舒 | 06-17 | **06-18** | 晚一天 |
| 2026-06-10 | 下下周五之前把检修单送过来 | 06-26 | **07-22** | 晚 26 天 |
| 2026-09-21 | 11 月 12 号前把 2027 年价目表定下来 | 2026-11-12 | **2027-11-12** | 晚 12 个月 |
| 2026-06-10 | 6 月 24 号之前把花材清单定下来 | 06-24 | 06-24 | 正确（原文写死日期） |

规律：原文写死「X 月 X 号」的碰巧对；含相对时间（下周/下下周）的全错。

**复现**：写入 `timestamp=2026-06-10 10:00:00` 的对话，抽取后查库：

```bash
curl -s -X POST :3103/api/memory/batch -d '{"items":[{"conversation":"下周三之前把端午排班表发出去。","timestamp":"2026-06-10 10:00:00"}]}'
sqlite3 data/shouxin.db "select title,deadline from todo_list where title like '%排班表%';"
# -> 2026-06-10T00:00:00+00:00   （应为 2026-06-17 23:59:59）
```

**根因**：

① 提示词无对话时间。`src/memory_extractor/extractor.rs:290-350` 的 `default_system_prompt()` 是纯静态字符串，全文没有当前日期。

② 模型日期只要 ≥ 对话当天就采信。`src/service/background_queue.rs:1020-1022`：

```rust
let model_ok = model_deadline.filter(|dt| {
    *dt >= ref_day_start && *dt < ref_time + chrono::Duration::days(365 * 5)  // 5 年等于不设限
});
```

③ 口径不一致。模型给 `00:00:00Z`，本地解析返回 `23:59:59`（`time_parser.rs:995` 的 `end_of_day`）。前者会让待办在截止日**凌晨**就过期。

**改法**：

1）把对话时间送进模型。`background_queue.rs` 构造 messages 处（约 325 行）：

```rust
let ref_time = task.conversation_time.unwrap_or_else(Utc::now);
let weekday = ["一","二","三","四","五","六","日"]
    [ref_time.weekday().num_days_from_monday() as usize];
let time_hint = format!(
    "【对话发生时间】{}（周{}）\n所有相对时间（今天/明天/本周X/下周X/下下周X/月底）都以该时间为基准，不要使用其他日期。\n\n",
    ref_time.format("%Y-%m-%d %H:%M"), weekday);
let messages = vec![Message::user(format!("{}{}", time_hint, task.conversation_text))?];
```

2）系统提示词加两条硬约束（`extractor.rs` 的「待办」小节）：

```
- deadline 必须晚于【对话发生时间】；相对时间一律以该时间为基准换算
- 无法确定具体日期时不要填 deadline，禁止猜测年份
```

3）模型与本地交叉校验。替换 `background_queue.rs:1006-1045` 的取值逻辑：

```rust
let local = crate::utils::time_parser::resolve_deadline_ex(   // 新增，见第 4 步
    &task.conversation_text, &extracted_todo.title, ref_time);

let model_deadline = extracted_todo.deadline.as_ref()
    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
    .map(|dt| dt.with_timezone(&Utc))
    .filter(|dt| *dt >= ref_day_start)                         // 不许早于对话当天
    .filter(|dt| *dt < ref_time + chrono::Duration::days(730))  // 5 年 → 2 年
    .map(crate::utils::time_parser::end_of_day);                // 统一 23:59:59

let deadline = match (model_deadline, local) {
    // 本地命中"显式截止"且与模型差 >1 天 → 本地优先
    (Some(m), Some(l)) if l.explicit
        && (m.date_naive() - l.at.date_naive()).num_days().abs() > 1 => l.at,
    // 模型原样返回对话当天，原文却有未来时间词 → 视为模型失败
    (Some(m), Some(l)) if m.date_naive() == ref_time.date_naive() => l.at,
    (Some(m), _) => m,
    (None, Some(l)) => l.at,
    (None, None) => crate::utils::default_deadline_from(ref_time, 7),
};
```

> `end_of_day` 目前私有（`time_parser.rs:995`），需改 `pub(crate)`。

4）让本地解析器区分「显式截止」与「猜的」。现有 `resolve_deadline_in_text` 无法区分"命中之前/前/截止"和"退回第一个时间表达"：

```rust
pub struct ResolvedDeadline { pub at: DateTime<Utc>, pub explicit: bool }
pub fn resolve_deadline_ex(text: &str, title: &str, reference: DateTime<Utc>)
    -> Option<ResolvedDeadline>;
// 旧函数保留，内部调用它并丢弃 explicit，避免破坏现有调用方
```

**验收**（上表 5 条都要落到「期望」列，另加）：

- 所有 `deadline` 时间部分为 `23:59:59`，且不早于对话日；
- `2026-10-20 下周三之前` → `2026-10-28`；
- `2026-08-03 下周三是供应商老陈承诺补货的最后期限` → `2026-08-12`（依赖 P1-5 一起改）。

**改动文件**：`src/service/background_queue.rs`、`src/memory_extractor/extractor.rs`、`src/utils/time_parser.rs`

---

## P1-2 事件描述：写进 summary 后在查询层被丢弃

**现象**：`/api/memory/search/events` 与 `/api/search/smart` 返回的结构化事件 `description` 恒为空；库里 `summary` 有完整描述。

```json
POST /api/memory/search/events {"query":"报关","limit":5,"scope":"user","user_id":"yezhiqiu"}
{ "title": "报关单被扣", "description": "", "time_info": "2026-12-17T00:00:00+00:00" }
```

**根因链**（三处，缺一不可）：

① 写路径（N-15 修复）把描述挪进 `summary`、`event_type` 写成空串 —— `background_queue.rs:861-863`：

```rust
summary: e.description.chars().take(200).collect(),
event_type: String::new(),
```

② 查询层把「空串」当成有效值，`summary` 被直接丢弃 —— `store.rs:1232`（`search_events_with_owner` 的行映射）：

```rust
// event_type 是 Some("")，不是 None，于是 unwrap_or_else 的兜底永远不触发
event_type: event_type_val.unwrap_or_else(|| summary.clone().unwrap_or_default()),
```

③ 读接口把这个字段当描述返回 —— `three_library_manager.rs:1241`：

```rust
description: trait_event.event_type.clone(),   // 恒为空
...
related_contacts: vec![],                       // 同样硬编码
```

`/api/contacts/:id/detail` 走另一条映射，所以那边能看到 `summary`，把问题掩盖了。

**改法**：

1）最小改动：把空串当成"没有值"，回退到 `summary`。改 `store.rs:1232` 一行，下游不用动：

```rust
event_type: event_type_val
    .filter(|s| !s.trim().is_empty())
    .or(summary.clone())
    .unwrap_or_default(),
```

> 如果不想让 `event_type` 继续兼表达"描述"，另一条路是给 `TraitEventIndex`（`traits/store_traits.rs:188`）加 `summary` 字段，但要同步改 3 处构造（`store.rs:1229`、`2131`、`2268`），改动面大得多。

2）`related_contacts` 从 `contact_ids` 解析。注意 `search_events_with_owner` 的 SELECT（`store.rs:1144`）根本没取 `contact_ids`，所以要先补一次批量查询，不要直接读结构体字段：

```rust
// store.rs 新增：SELECT e.event_id, c.name FROM event_index e
//                JOIN json_each(e.contact_ids) j JOIN contact_index c ON c.id = j.value
//                WHERE e.event_id IN (...)
pub async fn contact_names_by_event_ids(&self, ids: &[String])
    -> Result<HashMap<String, Vec<String>>, ShouxinError>;

// three_library_manager.rs：map 之前先查一次
let ids: Vec<String> = trait_events.iter().map(|e| e.event_id.clone()).collect();
let contacts = self.relational_store.contact_names_by_event_ids(&ids).await?;
...
related_contacts: contacts.get(&trait_event.event_id).cloned().unwrap_or_default(),
```

**验收**：

- `query=报关` → `description` 非空且等于库里 `summary`；smart 接口同样非空（同一函数，不必二次改）；
- 该事件 `related_contacts` 含「老陈」；
- 造一条 `event_type` 非空的旧数据，`description` 仍取 `event_type`，不被 `summary` 覆盖。

**改动文件**：`src/memory/three_library_manager.rs`、`src/stores/relational/store.rs`

---

## P1-3 姓名清洗把「王建国」截成「王建」

**现象**：官方冒烟语料写的是「本名**王建国**」，入库是「王建」，联系人任何字段（`name`/`aliases`/`notes`/`background`）都不再有「王建国」。搜「王建国」0 条，搜「王建」1 条。

```
[Name Fix] Truncated name detected: '王建国' -> '王建'
```

**根因**：`extractor.rs:645` 的单字后缀表含「国」，且 `while changed` 循环裁到不能再裁：

```rust
"国", "家", "城", "市", "县", "省", "区", "镇", "村",
```

任何以「国」结尾的三字名都会被砍（建国、卫国、志国、兴国）。同一张表里的「中/上/下/里/外/内/间/边/面/头/好/想/文/诗/画/歌/舞/得」同理。

**改法**：

1）把后缀表收缩成"虚词白名单"，只留明确不会出现在姓名末尾的字：

```rust
// 保留（单字）
"的","了","是","在","说","道","问","答","叫","把","将","被","让","请","对","向",
// 保留（多字，原样不动）
"今天","昨天","明天","现在","当时","后来","之前","之后","很厉","的人",
// 删除
"国","家","城","市","县","省","区","镇","村","中","上","下","里","外","内","间","边","面",
"头","好","想","文","诗","画","歌","舞","得","借","打","走","跑","跳","飞","游","吃","喝",
"睡","做","去","来","过","要","会","能","可","很","厉","害","和","与","或"
```

2）保留现有 `name.chars().count() > 2` 门槛（两字名永不裁）。
不要额外加"三字及以下不裁"的白名单——那会把「王建的」漏成「王建的」。

3）补回归测试（下表）。

> 可选加固：增加 `conversation: &str` 参数，若 `conversation.contains(name)` 直接返回。要动 5 个调用点（620/755/1262/1279/1501），按排期决定。

**验收**（补单测）：

| 输入 | 期望 |
|---|---|
| 王建国 / 李卫国 / 张兴国 / 欧阳建国 | 原样保留 |
| 李小明说 | 李小明 |
| 王建的 | 王建 |
| 小明的姐姐 | 不进联系人（走 `is_relational_phrase`） |

端到端：重跑冒烟后 `select name from contact_index where name like '王建%'` → `王建国`。

**改动文件**：`src/memory_extractor/extractor.rs`

---

## P1-4 文档推荐的合并端点：不改数据、不校验影响行数，还返回成功

**现象**：

① 不存在的 id 返回成功：

```json
POST /api/events/pending_merges/999999/resolve  {"action":"merge"}
{"success": true, "data": {"action":"merge","merge_id":999999,"resolved":true}}
```

② 更严重：**真实 id 也只改状态，不执行合并**。`resolve_pending_event_merge` 函数体只有一条 `UPDATE pending_event_merge`，不碰 `event_index`。真正的合并逻辑在 `confirm_pending_event_merge`（`store.rs:3478`），它合并完才回头调 `resolve_pending_event_merge` 收尾（`store.rs:3592`）。

而 `docs/API.md:357`、`docs/ARCHITECTURE.md:135` 都把 `/pending_merges/:id/resolve` 写成唯一出口，`/api/events/confirm_merge` 根本没进文档。**按文档确认合并，系统回 `resolved:true`，两条事件却仍然并排存在。**

`v0.7.3` 的修复只加在 `confirm_pending_event_merge` 上：

```json
POST /api/events/confirm_merge {"merge_id":999999,"action":"merge"}
{"success": false, "error": "实体未找到: 待确认合并记录不存在或已处理"}
```

修对了一个，用户按文档调的却是另一个；冒烟只用存在的 id 测 resolve，39/39 发现不了。

**根因**：`store.rs:3149-3172`：

```rust
query(r#"UPDATE pending_event_merge SET status='resolved', resolved_at=?, resolution_action=? WHERE id=?"#)
    .bind(...).bind(action).bind(merge_id)
    .execute(&self.pool).await?;
Ok(())          // 从不检查 rows_affected，也从不碰 event_index
```

**改法**：

1）补影响行数校验：

```rust
let result = query(...).execute(&self.pool).await?;
if result.rows_affected() == 0 {
    return Err(ShouxinError::EntityNotFound(
        format!("待确认合并记录不存在或已处理: {}", merge_id)));
}
```

2）路由按 action 分流，`merge`/`create` 走真正干活的函数（`routes.rs:1811`）：

```rust
match req.action.as_str() {
    "merge" | "create" => match async_memory.confirm_event_merge(merge_id, &req.action, None).await {
        Ok(event_id) => ApiResponse::success(json!({
            "merge_id": merge_id, "action": req.action, "resolved": true, "event_id": event_id
        })),
        Err(e) => ApiResponse::error_with_status(format!("处理失败: {}", e), StatusCode::BAD_REQUEST),
    },
    "dismiss" | "reject" | "approve" => match rel_store
        .resolve_pending_event_merge(merge_id, &req.action).await {
        Ok(()) => ApiResponse::success(json!({ "merge_id": merge_id, "resolved": true })),
        Err(e) => ApiResponse::error_with_status(format!("处理失败: {}", e), StatusCode::BAD_REQUEST),
    },
    _ => ApiResponse::error_with_status("无效的 action".to_string(), StatusCode::BAD_REQUEST),
}
```

3）文档只留一个出口，删掉另一处的矛盾描述。

**验收**：

- `999999` + `merge` → 4xx、`success:false`、信息含"不存在或已处理"；
- 真实 pending id + `merge` → 事件总数 -1（或源事件 `is_merged=1`），响应含 `event_id`；
- 同一 id 再调 → 4xx；
- 真实 id + `dismiss` → 状态 `resolved`，事件数不变；
- 冒烟补一条"不存在的 merge_id 必须非 2xx"。

**改动文件**：`src/stores/relational/store.rs`、`src/api/routes.rs`、`docs/API.md`、`docs/ARCHITECTURE.md`

---

## P1-5 两种常见句式仍算错截止日

**现象**：

**(a)「X 是……最后期限」取到句首的事件时间。** 对话时间 2026-08-03：

> 今天上午和何晓岚盘完账……**下周三是**供应商老陈承诺补货的**最后期限**

→ 落库 `2026-08-03`（当天），待办创建即 `auto_invalid`，用户永远看不到。

**(b)「下下周五」当「下周五」，早一周。** 2026-09-08（周二）说"下下周五之前" → 落库 `09-18`，应为 `09-25`。

**根因**：

(a) `time_parser.rs:919-937` 的截止标记只认 `之前/以前/前/截止`，且只看时间表达**紧跟**的内容。本例"下周三"后面是"是供应商……"，判定失败后退回"第一个能解析的时间"——句首"今天上午"。

(b) `time_parser.rs:580`：

```rust
let next_week = text.contains("下周") || text.contains("下星期");
```

"下下周五"包含"下周"，于是按 +7 天算。扫描器（正则 `下下周[一二三四五六日]`，`time_parser.rs:60`）明明切出了完整表达，解析器却退化了。

**改法**：

(a) 加句级截止词兜底，改用最后一个时间表达（插在"紧跟标记"循环之后、"第一个能解析"循环之前）：

```rust
const DEADLINE_WORDS: &[&str] = &["最后期限", "期限", "到期", "截止", "deadline", "必须完成"];
if DEADLINE_WORDS.iter().any(|w| sentence.contains(w)) {
    for (expr, _) in exprs.iter().rev() {   // 截止词在句尾时，真正的时间通常是最后一个
        if let Some(dt) = parse_event_time(expr, reference_time) {
            return Some(end_of_day(shift_this_week_if_past(expr, reference_time, dt)));
        }
    }
}
```

> 不要无条件改成"取最后一个"——会破坏"周六谈续约……下周一前给报价单"这类已修好的用例。只在出现截止词时切换。

(b) 显式处理 `下下周`：

```rust
let next2_week = text.contains("下下周") || text.contains("下下星期");
let next_week = !next2_week && (text.contains("下周") || text.contains("下星期"));
...
let days_until = if next2_week {
    (7 - days_since_monday + target_offset + 7).max(8)   // 下下周一 = 12 天后
} else if next_week {
    (7 - days_since_monday + target_offset).max(1)
} ...
```

顺带核对 `上上周` 分支（`time_parser.rs:583-589`）的 `target_offset - days_since_monday - weeks_back * 7` 是否有同样偏移，补对称用例。

**验收**：

| 对话时间 | 原话 | 期望 |
|---|---|---|
| 2026-08-03 | 今天上午盘完账……下周三是供应商补货的最后期限 | 2026-08-12 |
| 2026-09-08 | 下下周五之前把报价单发出去 | 2026-09-25 |
| 2026-06-10 | 下下周五之前把检修单送过来 | 2026-06-26 |
| 2026-06-10 | 下周三之前把排班表发出去 | 2026-06-17（回归，别被 `下下周` 分支带偏） |
| 2026-10-28 | 周六要和王海涛谈续约，下周一前给我报价单 | 2026-11-02（回归，仍取截止标记） |

**改动文件**：`src/utils/time_parser.rs`

---

## 3. 改完自检

```bash
cd E:/ShouXin
cargo test --lib                 # 基线 105 passed，新增用例后应为 105 + N
cargo build --release
BASE=http://127.0.0.1:3103 USER_ID=reextract bash scripts/api_smoke_test.sh
```

回归重点（上一轮已通过，别改坏）：

1. `history limit`：`query=芍药&limit=2` → 1 条，不因 limit 缩小而返回 0；
2. 无关查询阈值：`南极磷虾捕捞配额` → events / history / todos 全 0；
3. `contact_id` 过滤：何晓岚 → 2 条，`contact_ids` 均含她；
4. `本周X` 不指向过去：周日说"本周六" → 下周六。

## 4. 备忘

- P1-2、P1-3 是"改了一半"，P1-4 是"改错了端点"，P1-1、P1-5 是"只修了样例里的那一句"。
- 验收用例请写进单测，否则下一轮换句话又会退化。
