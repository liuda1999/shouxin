# 守心 v0.7.6 — 两个阻塞级问题的修复说明

> 面向修复。每条包含：实测现象 / 根因（代码 + 行号）/ 改法（可直接粘的 Rust）/ 验收（可断言）。
> 实测环境：`E:/tmp/t1/`（独立 data_dir，端口 3110），17 条真实对话 + 1 个 txt。
> 版本 v0.7.6，官方冒烟 `PASS=39 FAIL=0 SKIP=1` 通过——这两个问题冒烟测不出来。

---

## B-1 待确认合并在四个层面上失效

### 现象

17 条对话产生了 5 条待确认合并，**目标事件集中在两条上，源事件之间毫无关联**：

| merge id | 源事件摘要 | 目标事件 | 相似度 |
|---|---|---|---|
| 1 | 写成可以直接照着改的修复清单 | 用户让我把守心 v0.7.3 的测试报告精简一下 | 0.904 |
| 3 | 用户把项目更新到了 0.7.6 | *同上* | 0.914 |
| 4 | 用户要求我忘掉之前测试踩过的坑 | *同上* | 0.903 |
| 2 | 清单写完了，放在 docs/FIX_P0_P1_v0.7.3.md | *同上* | 0.909 |
| 5 | 11 月 12 号前把明年的预算表定下来 | 醒我 6 月 24 号之前把年中总结交上去 | 0.906 |

相似度全部落在 **0.902–0.914** 这个窄带里，内容跨度却从"预算表"到"测试报告"。

然后点合并：

```
POST /api/events/confirm_merge {"merge_id":5,"action":"merge"}
  -> 200 {"success":true,"message":"事件已合并","event_id":"5502fb90-..."}

目标事件 5502fb90 的 summary：合并前后完全一致（没被追加）
event_index 条数：10 -> 10
```

接口说"事件已合并"，目标事件一个字都没变。

走 `create` 分支则是另一番景象——会插入一条 **`title` 为 `NULL`** 的事件（库里 `92be925a`）：

```
event_id=92be925a...  title=NULL
summary="写成可以直接照着改的修复清单 写成可以直接照着改的修复清单。"
```

标题重复了两遍，标题列为 NULL，事件列表里显示成空白。

### 根因：四层叠加，每层单独都能让合并失效

---

**第 1 层 — 阈值有两套，配置里那套不生效**

待确认区由两条路径写入，阈值各不相同：

```rust
// src/service/background_queue.rs:111 —— 读配置（config.toml:128-129: 0.96 / 0.90）
*MERGE_THRESHOLDS.get().unwrap_or(&(0.96, 0.90))

// src/memory/three_library_manager.rs:664 / 681 —— 硬编码，不读配置
if similarity > 0.90 { /* 自动合并 */ }
else if similarity > 0.75 { /* 进待确认区 */ }
```

事件实际落库走的是后者。**改 `config.toml` 里的 `event_merge_pending_threshold` 对事件合并没有任何影响。**

日志里 `[P1-7] 待确认合并: ... (阈值 0.90-0.96)` 打印的是 queue 那套，会让人误以为配置生效了。

阈值只在启动时注入给 queue 一处（`src/shouxin-server.rs:497` → `set_merge_thresholds`），`three_library_manager` 没有对应的注入口，永远走 `unwrap_or` 都拿不到的硬编码值。

`background_queue.rs:100-101` 的注释说明这个问题被调过一次，但只改了一半：

```rust
/// 本来就有 0.84–0.89（实测），旧的 0.75/0.90 阈值会把"完全不同的事件"判成相似，
/// 导致新事件全部堆进待确认区、一条都落不了库。
```

——阈值从 0.75/0.90 提到了 0.90/0.96，但 `three_library_manager.rs` 里那一份仍是 **0.90/0.75 的旧值**。本次实测 5 条全部落在 0.902–0.914，正是"旧阈值下完全不同的事件被判成相似"的原样复现。

---

**第 2 层 — 进待确认区后 `continue`，源事件根本没创建**

```rust
// src/service/background_queue.rs:786-808
} else {
    // 存入待确认合并区 (0.75 < score <= 0.90)
    let source_event_id = format!("pending_{}", uuid::Uuid::new_v4());   // 788
    match state.relational.insert_pending_event_merge(
        &source_event_id,                                                // 假 id
        ...
    ).await {
        Ok(merge_id) => { tracker.merge_ids.push(merge_id); continue; }  // 807 直接跳过创建
```

`src/memory/three_library_manager.rs:689` 同样有一份 `format!("pending_{}", uuid::Uuid::new_v4())`，`:701` 同样是 `continue`。

后果：**待确认区里的"源事件"在 `event_index` 中不存在**。用户此时无论点"合并"还是"不合并"，这条记忆都不在库里——点"不合并"等于永久丢弃。

---

**第 3 层 — 合并内容的写入条件恒为假（这是"什么都没发生"的直接原因）**

```rust
// src/stores/relational/store.rs:3753-3757
if let Some(desc) = additional_description {
    if !event.event_type.is_empty() && !desc.is_empty() {
        event.event_type = format!("{}\n---\n合并自: {}", event.event_type, desc);
    }
}
```

条件是 `!event.event_type.is_empty()`，但写入路径把 `event_type` 写死成空串：

```rust
// src/service/background_queue.rs:881
event_type: String::new(),
```

实测全表 10 条事件 `event_type` **全部为空字符串**。于是这个 `if` 永远不成立，合并摘要永远写不进去，只剩下一句 `UPDATE ... SET last_merge_check_at = ?`：

```
目标事件 b7c54a01 被 merge 了 3 次：
  event_type      = ''          （恒空）
  summary         = 未变
  last_merge_check_at = 2026-11-01T15:39:09   （只有这个被刷新）
```

顺带一处语义冲突：`background_queue.rs:878` 的注释写着"`event_type` 留空（事件没有类型维度）"，而合并逻辑偏偏往 `event_type` 里塞"合并自: ..."。即便条件成立，内容也会写进一个被声明为恒空的列。

而且用户看到的描述来自 `summary` 列（读路径 `three_library_manager.rs` 的 `description`），所以就算写进 `event_type`，用户也看不到。

---

**第 4 层 — 用来算相似度的文本把标题说了两遍**

两处各有一份，都要改：

```rust
// src/memory/three_library_manager.rs:640-644
// src/service/background_queue.rs:667-671   （同一段，注释写着"与同步路径一致"）
let event_summary_for_merge = format!("{} {}", e.title.as_deref().unwrap_or(""), e.description);
```

`description` 常常以 `title` 开头，拼出来就是：

```
"写成可以直接照着改的修复清单 写成可以直接照着改的修复清单。"
"11 月 12 号前把明年的预算表定下来 11 月 12 号前把明年的预算表定下来。"
```

自己跟自己重复，会抬高短文本的向量相似度——这是 5 条不相关事件全部挤到 0.90 以上的一部分原因。

---

### 改法

按 1→4 顺序做，1 和 4 解决"误报太多"，2 和 3 解决"点了没反应"。

**① 阈值统一（必做，改 2 处）**

把 `MERGE_THRESHOLDS` 提到两处都能读到的位置（例如 `src/utils/` 或 `src/config/`），让 `three_library_manager.rs:664/681` 也走它：

```rust
// src/memory/three_library_manager.rs
let (auto_th, pending_th) = crate::service::background_queue::merge_thresholds();
// 需要把 background_queue.rs:106 那个私有 fn 改成 pub(crate)

if similarity > auto_th {
    // 自动合并
} else if similarity > pending_th {
    // 进待确认区
}
```

同时把 `:684` 日志里的硬编码 `(0.75-0.90)` 改成打印 `pending_th`/`auto_th`，免得再误导。

**② 合并内容写进 `summary`，去掉恒假条件（必做，改 1 处）**

```rust
// src/stores/relational/store.rs:3753-3757 替换为
if let Some(desc) = additional_description {
    let desc = desc.trim();
    if !desc.is_empty() && !event.summary.contains(desc) {
        event.summary = format!("{}\n---\n合并自: {}", event.summary, desc);
    }
}
```

并把同函数 `:3759-3771` 的 SQL 从写 `event_type` 改成写 `summary`：

```rust
query(
    r#"UPDATE event_index
       SET contact_ids = ?, summary = ?, last_merge_check_at = ?
       WHERE event_id = ?"#,
)
.bind(&event.contact_ids)
.bind(&event.summary)      // 原为 &event.event_type
.bind(Utc::now().to_rfc3339())
.bind(event_id)
```

**③ 让合并结果可验证（必做，改 1 处）**

`update_event_after_merge` 现在返回 `Result<()>`，调用方无法知道到底改没改。改成返回影响行数：

```rust
pub async fn update_event_after_merge(...) -> Result<u64, ShouxinError> {
    // ...
    let r = query(...).execute(&self.pool).await?;
    Ok(r.rows_affected())
}
```

`confirm_pending_event_merge` 的 merge 分支（`store.rs:3599`）据此判断：

```rust
let affected = self.update_event_after_merge(target_id, &source_contact_ids, Some(&source_summary)).await?;
if affected == 0 {
    return Err(ShouxinError::EntityNotFound(
        format!("目标事件 {} 不存在，合并未生效", target_id)));
}
```

这样"合并成功但什么都没改"会如实报错，而不是返回"事件已合并"。

**④ 补齐 `create` 分支的标题（必做，改 1 处）**

```rust
// src/stores/relational/store.rs:3623-3638，create 分支
let summary = source_event_summary.unwrap_or(&source_summary);
let title = summary
    .split(['。', '！', '？', '\n'])
    .next()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());
// ...
summary: summary.to_string(),
title,                                   // 原为 None
```

**⑤ 去掉相似度文本的重复（建议，改 1 处）**

```rust
// src/memory/three_library_manager.rs:640-644
// src/service/background_queue.rs:667-671（同改）
let title = e.title.as_deref().unwrap_or("").trim();
let desc  = e.description.trim();
let event_summary_for_merge = if !title.is_empty() && desc.starts_with(title) {
    desc.to_string()
} else {
    format!("{} {}", title, desc)
};
```

**⑥ 源事件先进库再进待确认区（可选，改动较大但更彻底）**

若采纳，`background_queue.rs:807` 和 `three_library_manager.rs:701` 的 `continue` 改成先落库、拿到真实 `event_id` 再 `insert_pending_event_merge`，`:788`/`:689` 的 `pending_{uuid}` 即可删除。这样：源事件不会因为用户"不合并"而丢失；`confirm_pending_event_merge:3584` 那段 `if !source_event_id.starts_with("pending_")` 的特判也能一并删掉；`:3606` 的 `update_event_merge_target` 才能真的标记 `is_merged`。

代价是待确认区期间库里会多一条事件，需要 UI 上区分。按排期决定。

### 验收

准备数据（全新库，写入 4 条内容互不相干的对话）：

```bash
curl -s -X POST http://127.0.0.1:3110/api/memory/batch -H 'Content-Type: application/json' \
 -d '{"user_id":"fix","scope":"user","items":[
   {"conversation":"用户让我把测试报告精简一下","timestamp":"2026-11-01 10:00:00"},
   {"conversation":"11 月 12 号前把明年的预算表定下来","timestamp":"2026-11-01 10:05:00"},
   {"conversation":"供应商说冷藏柜要检修","timestamp":"2026-11-01 10:10:00"},
   {"conversation":"王建国说下周一之前把方案发给我","timestamp":"2026-11-01 10:15:00"}]}'
```

断言：

| # | 断言 | 修前 |
|---|---|---|
| 1 | 待确认区条目 ≤ 1（4 条互不相关，理想为 0） | 3–5 条误报 |
| 2 | `pending_event_merge.source_event_id` 若非 `pending_` 前缀，必能在 `event_index` 查到 | 全是 `pending_` |
| 3 | 合并一条后，目标事件 `summary` 含 `\n---\n合并自: ` 字样 | 完全没变 |
| 4 | 合并影响行数 > 0；目标不存在时接口报错而非返回成功 | 恒返回成功 |
| 5 | 走 `create` 产生的事件 `title IS NOT NULL` | `title = NULL` |
| 6 | 改 `config.toml` 的 `event_merge_pending_threshold` 为 0.95 后重启，待确认区条目数随之变化 | 无变化 |

直接查：

```bash
sqlite3 E:/tmp/t1/data/shouxin.db \
 "select id, source_event_id, target_event_id, round(similarity_score,3) from pending_event_merge;"
sqlite3 E:/tmp/t1/data/shouxin.db \
 "select event_id, title, substr(summary,1,60), event_type from event_index;"
```

---

## B-2 兜底抽取的事件标题从词中间切开

### 现象

原文：`提醒我 6 月 24 号之前把年中总结交上去。`

```
落库事件 title = "醒我 6 月 24 号之前把年中总结交上去"   ← 开头的"提"被吃掉
同一句话抽出的待办 title = "年中总结"                    ← 待办是对的
```

只有事件标题坏。触发条件是模型返回 `events: []` 走兜底——这条对话的日志：

```
[LLM Extract] Raw response: {"people":[],"events":[],"todos":[{"title":"年中总结",...}]}
```

### 根因

```rust
// src/memory_extractor/extractor.rs:1621-1639
fn event_title_window(chars: &[char], kw_start: usize, kw_len: usize) -> (usize, usize) {
    const STOP: [char; 9] = ['，', '。', '；', '！', '？', '：', '\n', '…', '、'];
    const MAX_LEFT: usize = 16;
    let mut start = kw_start.saturating_sub(MAX_LEFT);          // 1626 先硬回退 16
    if let Some(pos) = (start..kw_start).rev().find(|&i| STOP.contains(&chars[i])) {
        start = pos + 1;                                        // 1627 只在 16 字窗口内找句读
    }
    ...
}
```

关键词是"总结"，位于第 17 字（0-based）。`17 - 16 = 1`，窗口 `[1, 17)` 里没有句读，于是 `start` 停在 1 → 从第 1 字起 → "醒我…"。

注释（`:1624-1631`）写着这个问题已处理过——"不要再固定左移 8 字，8 字会正好切在词中间"。**但改法只是把 8 换成 16，机制没变**：只要关键词距句首超过 16 字且中间无标点，照样从半个词开始。

注意 `:1627` 的搜索范围被限制在 `(start..kw_start)`，即只向左看 16 字。句首到关键词之间超过 16 字时，即使更左边有句号也找不到——这才是真正的 bug：不是"回退多少"的问题，是**搜索范围被 MAX_LEFT 截断**了。

### 改法

把搜索范围放开到整句开头，找不到句读就从句子起头取（标题长一点没关系，从词中间切才是硬伤）：

```rust
fn event_title_window(chars: &[char], kw_start: usize, kw_len: usize) -> (usize, usize) {
    const STOP: [char; 9] = ['，', '。', '；', '！', '？', '：', '\n', '…', '、'];
    const MAX_TITLE: usize = 32;   // 只限制总长，不再限制左视距离

    // 向左回到最近的句子边界；搜索范围是整句开头，不再被固定窗口截断
    let mut start = (0..kw_start)
        .rev()
        .find(|&i| STOP.contains(&chars[i]))
        .map(|p| p + 1)
        .unwrap_or(0);
    while start < kw_start && STOP.contains(&chars[start]) {
        start += 1;
    }
    // 标题总长超上限时，从右侧让位改为从左侧收窄会再次切到词中间，
    // 所以这里只截右端（end 侧），保持 start 不动
    let mut end = (kw_start + kw_len).min(chars.len());
    while end < chars.len() && end - kw_start < 24 && !STOP.contains(&chars[end]) {
        end += 1;
    }
    if end - start > MAX_TITLE {
        end = start + MAX_TITLE;
    }
    (start, end)
}
```

改动要点只有一句：`.rev().find(...)` 的迭代区间从 `(kw_start - 16 .. kw_start)` 改成 `(0 .. kw_start)`，`unwrap_or(0)` 兜底。

`MAX_LEFT` 常量随之删除（`grep MAX_LEFT src/` 确认只有这一处引用）。

### 验收

```bash
curl -s -X POST http://127.0.0.1:3110/api/memory/batch -H 'Content-Type: application/json' \
 -d '{"user_id":"fix2","scope":"user","items":[
   {"conversation":"提醒我 6 月 24 号之前把年中总结交上去。","timestamp":"2026-06-10 10:00:00"},
   {"conversation":"今天又和张伟确认了预算表，下周一之前要交。","timestamp":"2026-06-10 10:05:00"},
   {"conversation":"我让龙飞整理一下季度评估材料。","timestamp":"2026-06-10 10:10:00"}]}'
```

| 原话 | 期望标题 | 修前 |
|---|---|---|
| 提醒我 6 月 24 号之前把年中总结交上去。 | 以「提醒我」开头 | 「醒我 6 月 24 号…」 |
| 今天又和张伟确认了预算表，下周一之前要交。 | 以「今天又和」开头 | 从词中间起 |
| 我让龙飞整理一下季度评估材料。 | 以「我让龙飞」开头 | 从词中间起 |

这三条的关键词（总结 / 预算 / 评估）距句首都不足 16 字或有逗号，改动前后都应对；真正区分改法好坏的是**关键词距句首超过 16 字且中间无标点**的句子，例如：

```
"上周三我和供应商的老陈一起开了整整两个小时的季度复盘会议，最后决定延期。"
```

期望标题以「上周三」开头（关键词"会议"在第 30 字左右）。修前会从第 14 字左右切开。

```bash
sqlite3 E:/tmp/t1/data/shouxin.db "select title from event_index order by created_at desc limit 3;"
```

---

## 回归重点

改完 B-1 后请确认这四件事没被带坏（它们之前是好的，本次实测通过）：

1. 时间解析：以 2026-06-10 为基准，「下周三之前」→ 06-17、「下下周五之前」→ 06-26、09-21 说的「11 月 12 号前」→ 11-12
2. 人名：王建国 / 李小明 / 张伟明 / 陈静 完整入库，不截断
3. `confirm_merge` 传不存在的 `merge_id` 仍报"待确认合并记录不存在或已处理"
4. `bash scripts/api_smoke_test.sh` 仍 `PASS=39 FAIL=0`

---

## 附：两条问题的关联

B-2 产出的坏标题会流进 B-1：本次测试里，事件 `醒我 6 月 24 号之前把年中总结交上去` 被当成"11 月 12 号前把明年的预算表定下来"的合并目标（相似度 0.906）。建议先修 B-2 再验 B-1，否则待确认区里会混着坏标题带来的额外噪音。
