# 待修问题清单（P1）— v0.7.7

生成时间：2026-11-02 · 版本 v0.7.7 · 证据库 `E:/tmp/use/data/shouxin.db`（未触碰仓库 `data/`）

## 判定前提

**本系统的唯一调用方是智能体，终端用户不直接接触本系统。** 本文档所有判断都建立在这条前提上。

因此判据不是"用户会不会困惑"，而是：

> **智能体按 API 契约写入后，能否用同一套属主参数完整、干净地取回自己写入的数据及其派生数据。**

按这条判据，"问句『我还有哪些待办』返回 0 条"**不算问题**——智能体不会那样提问，它自己组织关键词。已确认排除，不在本文档内。

测试时用 `user_id=me`，只是**模拟"智能体在请求中显式携带 `user_id`"这一调用方式**，不是说有个人在操作。

**未发现 P0**：无服务不可用、崩溃、越权或不可恢复的数据丢失。

| 编号 | 一句话 | 判据上为什么算问题 | 改动量 |
|---|---|---|---|
| P1-1 | 心跳产物无视调用方属主，硬编码 `default_user` | 智能体带 `user_id` 写入的对话，其派生数据落到另一个属主空间，按同一 `user_id` 检索取不回 | 小 |
| P1-2 | 澄清类待办混进普通待办列表，且一次分析生成两条 | 智能体拉到的待办列表里混着系统自述文本，且两条分属不同空间 | 中 |
| P1-3 | 批量写入部分失败，顶层仍返回 `success: true` | 程序化调用方只看顶层 `success`，会认为全部写入成功 | 小 |

---

## P1-1 心跳产物无视调用方属主，硬编码 `default_user`

### 现象

智能体以 `user_id=me` 写入对话并触发心跳后，库里 10 条待办中有 **2 条落在 `default_user` 名下**：

| 数据来源 | 请求是否带 `user_id` | 落库 `owner_id` | `todo_type` | 按 `user_id=me` 能否取回 |
|---|---|---|---|---|
| 心跳澄清待办「需要澄清：SelfContradiction」 | 智能体写入对话时带了 `me` | `default_user` | ambiguity | **否** |
| 手动 `POST /api/todos` 照 `API.md:322` 示例 | 未带 | `default_user` | normal | **否** |

两行的性质不同，需要分开看：

- **第一行是缺陷**：智能体明确声明了属主，系统读取这些属主为 `me` 的对话后，把分析产物写到了另一个空间。智能体按同一 `user_id` 检索时，这段记忆的派生结果永远取不回。
- **第二行是契约不一致**：属主缺省时静默使用内置属主，本身可以是一种设计（单属主部署下完全自洽）。问题在于 `API.md:322` 的示例和 `scripts/api_smoke_test.sh:98-99` 都沿用"不带 `user_id`"的写法，而读取接口又要求带——按文档集成出来的智能体会写入一个它自己检索不到的空间。

**触发条件**：智能体在请求中显式携带非默认 `user_id`。若部署形态是单属主（全程不带 `user_id`，一切都在 `default_user`），则不受影响。

### 根因

**A. 心跳 `AskUser` 支路硬编码**（`src/heartbeat/executor.rs:241-243`）

```rust
let todo = crate::stores::relational::TodoItem::new(
    question.id.clone(),
    question.related_contact_ids.clone(),
    "default_user".to_string(),          // <- 写死，与调用方属主无关
    crate::core::contact::OwnerType::User,
    ...
```

**B. `Question` 结构没有属主字段，是 A 的直接原因**（`src/heartbeat/types.rs:17-28`）

只有 `id / priority / category / title / detail / context / created_at / expires_at / related_contact_ids / related_event_ids`，没有 `owner_type / owner_id`，executor 无从得知。

**C. 关键旁证：不是拿不到，是没传。** 同一个心跳模块里两处都取对了属主：

- `executor.rs:148-180`：提醒类待办走 `get_all_todo_owners()` 遍历真实属主；
- `analyzer.rs:1201`：`create_ambiguity_todo` 用 `recent_convs.first().map(|c| c.owner_id.clone())` 取到了 `me`。

运行时手里有属主信息，只有 `AskUser` 这条支路把它丢了。

**D. 手动创建路径的缺省兜底**（`src/api/routes.rs:2436-2439`）

```rust
Scope::User => (
    crate::core::contact::OwnerType::User,
    req.user_id
        .unwrap_or_else(|| crate::core::owner::DEFAULT_USER_ID.to_string()),  // <- 静默兜底
),
```

`DEFAULT_USER_ID = "default_user"`（`src/core/owner.rs:15`）。不报错、不告警。

### 改法

**①（必做）`Question` 增加属主字段，executor 使用真实属主**

```rust
// src/heartbeat/types.rs:17 结构体增加
pub owner_type: crate::core::contact::OwnerType,
pub owner_id: String,
```

三处构造点需同步补（`analyzer.rs:1166`、`analyzer.rs:1215`、`analyzer.rs:710` 附近）。
`analyzer.rs:1201` 已经算出了 `owner_id`，直接复用；`1166` 的 fallback 分支同函数内也有 `recent_convs`，取法一致。

```rust
// src/heartbeat/executor.rs:243-244 改为
question.owner_id.clone(),
question.owner_type,
```

**②（必做）文档与冒烟脚本对齐**（解决 D 的契约不一致）

`API.md:322` 示例补上属主字段，并加一句说明：

```json
{ "title": "整理下季度客户反馈", "deadline": "2026-11-02T23:59:59+08:00",
  "importance": "high", "user_id": "alice", "scope": "user" }
```

> 不带 `user_id` 时不会报错，待办归属内置属主 `default_user`；读取时须使用同一属主参数才能取回。

同步改 `scripts/api_smoke_test.sh:98-99`，否则冒烟会继续固化"写入不带属主"的用法。

**③（推荐，一行）缺省兜底留痕**

```rust
// src/api/routes.rs:2438 前插入
if req.user_id.is_none() {
    tracing::warn!("POST /api/todos 未指定 user_id，待办归属默认属主 default_user");
}
```

**④（可选，有破坏性）强制 `user_id`**

新增配置 `[api] require_user_id`（默认 `false`），开启后缺失 `user_id` 返回 `400`。
破坏面：冒烟脚本第 98 行、以及所有"单属主只用默认属主"的集成方式都会失败。**不建议默认开启**，仅作为多属主部署时的可选项。

### 验收

```
1. 智能体带 user_id=alice 写入对话，触发一次心跳澄清
   -> todo_list 新增记录的 owner_id == "alice"，不再出现 default_user
2. select owner_id, todo_type from todo_list where todo_type='ambiguity';
   -> 结果中不应出现 default_user
3. POST /api/todos body 不带 user_id
   -> data.owner_id == "default_user"，且日志出现 ③ 的 warn
4. POST /api/todos body 带 user_id=alice
   -> data.owner_id == "alice"；GET /api/todos?user_id=alice 能取回
5. GET /api/todos?user_id=alice
   -> 不含任何 owner_id != "alice" 的条目
6. 冒烟：bash scripts/api_smoke_test.sh -> PASS 数不下降
```

---

## P1-2 澄清类待办混进普通待办列表，且一次分析生成两条

### 现象

一段对话触发歧义检测后，待办表里出现**两条**记录：

| `owner_id` | `todo_type` | `entity_type` | 标题 | 说明 |
|---|---|---|---|---|
| `me` | `normal` | NULL | `澄清: 在最后一条对话中，实体"读的书"和"我已经买了八本"的指代对象不明确……`（**130 字整段分析**） | 与普通待办**无法区分** |
| `default_user` | `ambiguity` | `ambiguity` | `需要澄清：SelfContradiction` | 属主错（见 P1-1） |

对智能体的直接影响：`GET /api/todos?user_id=me` 返回 7 条，其中一条是 130 字的系统自述，和「举办读书会」「报告最终人数」并排返回。**由于 `todo_type=normal`，没有任何字段能把它筛掉**——智能体无法用契约内的手段区分"真正要做的事"和"系统的自言自语"。

### 根因

**① `create_ambiguity_todo` 没打类型标记**（`src/stores/relational/store.rs:3080-3089`）

```rust
let todo = crate::stores::relational::todo_list::TodoItem::new(...)
    .with_description(context_summary.to_string());
// 缺 .with_todo_type(...)，TodoItem::new 默认 todo_type = "normal"（todo_list.rs:88）
```

上一行刚往 `ambiguity_log` 插入记录并拿到 `ambiguity_id_int`，却没把它和这条 todo 关联。

**② 两条路径重复建 todo**

- `analyzer.rs:1202` → `create_ambiguity_todo()` 建第一条；
- 紧接着 `analyzer.rs:1215` 返回 `AskUser(Question)` → `executor.rs:234-255` 再建第二条。

同一次检测，两条 todo，且分属不同空间。

**③ 第二条 todo 的闭环本来就是断的，可以安全删除**

`resolve_question`（`question_pool.rs:88`）对非数字 id 走 `store.rs:3028-3047` 分支，按 `ambiguity_log.resolved_todo_id` 更新。第二条 todo 从未写入 `ambiguity_log`（executor 只调了 `add_todo`），因此 `rows_affected == 0`，代码里仅一行 `debug!`，静默失败。

问题本身仍完整保存在 question pool，`GET /api/heartbeat/pending_questions` 照常可见——删掉 todo 不损失任何信息。

**④ 列表接口无法按类型过滤**

`TodoFilter`（`src/api/async_memory.rs:29-38`）只有 `status / contact_id / limit / include_archived`；`ListTodosRequest`（`routes.rs:390-405`）也没有 `todo_type`。数据库里有 `idx_todo_type` 索引（`store.rs:570`），但没有任何查询用得上。

### 改法

**①（必做）给澄清待办打标记并关联闭环**

```rust
// src/stores/relational/store.rs:3088-3089
.with_description(context_summary.to_string())
.with_todo_type("ambiguity".to_string())
.with_entity("ambiguity".to_string(), ambiguity_id_int.to_string());
```

**②（必做）删掉重复的第二条**

删 `src/heartbeat/executor.rs:240-256` 整段 todo 构造与 `add_todo`，只保留 `question_pool.add_question(...)`。

依据根因 ③：这条 todo 属主错、无法闭环、信息冗余。删除后 `idx_todo_type`（`todo_type = 'ambiguity'`）正好只命中 ① 打过标记的记录，语义干净。

**③（必做）列表支持按类型过滤**

```rust
// 1) src/api/async_memory.rs:29  TodoFilter 增加
pub todo_type: Option<String>,
//    new() 初始化 None，并加 builder：
pub fn with_todo_type(mut self, todo_type: String) -> Self { self.todo_type = Some(todo_type); self }

// 2) src/api/routes.rs:390  ListTodosRequest 增加
pub todo_type: Option<String>,
//    routes.rs:2519 list_todos 里透传（照 status 的写法加一段）

// 3) src/api/async_memory.rs:552 调用处透传
filter.todo_type.as_deref(),

// 4) src/stores/relational/todo_list.rs:423 get_all_todos 增加参数 todo_type: Option<&str>
//    动态 SQL 追加（照已有 status/contact_id 的拼法）：
if let Some(t) = todo_type {
    sql.push_str(" AND todo_type = ?");
    // 与其余条件一样走 bind，避免字符串拼接
}
```

签名变更需同步 3 个调用点：`store.rs:1635`、`async_memory.rs:552`、`background_queue.rs:1172`（后两处传 `None`）。

**默认行为建议保持不变**（不自动排除 ambiguity），让智能体显式传 `todo_type=normal` 来拿"真正要做的事"。若你更在意开箱即得的干净列表，可再加一个 `exclude_todo_types` 参数。

**④（可选）标题别塞整段分析**

`store.rs:3085` 的 `format!("澄清: {}", description)` 把整段分析当标题。建议截断，完整内容留在 `description`：

```rust
let short: String = description.chars().take(30).collect();
format!("澄清: {}", short)
```

### 验收

```
1. 触发一次歧义检测
   -> todo_list 只新增 1 条 todo_type='ambiguity' 的记录（不再有第二条）
2. select owner_id from todo_list where todo_type='ambiguity';
   -> 不应出现 default_user（依赖 P1-1 ① 已修）
3. GET /api/todos?user_id=me&todo_type=normal
   -> 不含任何 todo_type='ambiguity' 的条目，也不含 130 字自述文本
4. GET /api/todos?user_id=me（不带 todo_type）
   -> 仍返回全部，行为与修复前一致（向后兼容）
5. 闭环：POST /api/heartbeat/resolve_question {"question_id":..., "answer":"..."}
   -> ambiguity_log 对应记录 status='resolved'、resolved_at 非空（不再 rows_affected=0）
6. 冒烟：PASS 数不下降
```

---

## P1-3 批量写入部分失败，顶层仍返回 `success: true`

### 现象

一次 `POST /api/memory/batch` 提交 6 条，实际接受 2 条：

```json
{ "success": true, "data": { "accepted": 2, "failed": 4, "items": [ ... 每条各自的 error ... ] } }
```

顶层 `success: true`，失败只体现在 `items[].error`。

**为什么在"调用方是智能体"的前提下这是 P1**：程序化调用方通常只看顶层 `success` 决定是否重试。本例中 4 条对话静默未入库，智能体却认为全部写完了——**这是数据丢失而不自知**，比"用户没注意到"严重。

### 根因

本次失败原因是时间戳在未来（当天 11-02，传了 11-03/11-04）。校验在逐条处理时执行，结果只写进对应 item，不上浮到顶层。
批处理不该因单条失败整体失败——这个设计是对的，错的是顶层语义。

### 改法（任选其一，都很小）

1. `failed > 0` 时顶层 `success` 置 `false`（最直白，但会改变现有语义，需确认无调用方依赖）；
2. 顶层加 `partial: true` 与 `message: "6 条中 4 条失败，详见 items[].error"`（兼容，推荐）；
3. 未来时间戳的错误文案写明边界（当前只说失败，不说明原因），便于智能体自动修正后重发。

### 验收

提交混合了合法与未来时间戳的批次 → 不看 `items` 也能从顶层判断出"存在失败"。

---

## 附：复现与自检

独立实例（不碰仓库 `data/`）：

```bash
mkdir -p /e/tmp/use
sed -e 's|^port = 3000|port = 3120|' -e 's|^data_dir = "./data"|data_dir = "E:/tmp/use/data"|' \
    config.toml > /e/tmp/use/config.toml
SHOUXIN_CONFIG=E:/tmp/use/config.toml ./target/release/shouxin-server.exe > /e/tmp/use/server.log 2>&1
```

P1-1 契约不一致复现（两行）：

```bash
curl -s -X POST http://127.0.0.1:3120/api/todos -H 'Content-Type: application/json' \
  -d '{"title":"整理下季度客户反馈","deadline":"2026-11-05T23:59:59+08:00","importance":"high"}' \
  | python3 -c "import sys,json;print(json.load(sys.stdin)['data']['owner_id'])"   # -> default_user
curl -s "http://127.0.0.1:3120/api/todos?scope=user&user_id=me&limit=50" | grep -c "整理下季度客户反馈"  # -> 0
```

自检 SQL：

```sql
-- P1-1: 不应再有落到 default_user 的澄清待办
select owner_id, todo_type, substr(title,1,40) from todo_list where owner_id='default_user';

-- P1-2: 澄清待办必须有类型标记，且每条 ambiguity_log 只对应一条 todo
select todo_type, count(*) from todo_list group by todo_type;
select count(*) from todo_list t join ambiguity_log a on a.resolved_todo_id = t.todo_id;
```

### 回归重点

1. `GET /api/todos` 不传 `todo_type` 时行为不变；
2. 心跳提醒类待办（`todo_type='reminder'`）仍按真实属主生成；
3. `GET /api/heartbeat/pending_questions` 条目数不受删除 executor todo 影响；
4. `POST /api/memory/batch` 全部成功时顶层 `success` 仍为 `true`；
5. 官方冒烟 `bash scripts/api_smoke_test.sh` PASS 数不下降。
