# HTTP API

所有接口返回 JSON。`owner` 相关接口需要 `scope` + `user_id`（或 `owner_type` + `owner_id`）定位命名空间，
跨命名空间访问一律返回"不存在"（不泄露资源是否存在）。

**错误表达约定**：业务错误大多走 **HTTP 200 + `{"success": false, "error": "..."}`**，
而不是靠 HTTP 状态码。只有参数校验失败、路由不存在、请求超时等框架级错误才用 4xx/5xx。

基础地址：`http://127.0.0.1:3000`

---

## 健康检查

```http
GET /health
```

```json
{ "status": "ok", "time": "2026-10-28T15:52:44.832070400+00:00" }
```

---

## 记忆写入

### 写入一段对话

```http
POST /api/memory/add
Content-Type: application/json

{ "conversation": "下周三和张总开会讨论预算，下周五前给出方案。", "user_id": "default_user" }
```

```json
{ "success": true, "data": { "contacts": [], "events": [], "learnings": [], "conversation_id": "33a67bf0-..." } }
```

**这是异步接口**：立即返回 `conversation_id`，抽取在后台队列进行。
刚返回时 `contacts/events/learnings` 通常为空数组，属于正常。

**指定对话发生时间（导入历史必填）**：

```json
{
  "conversation": "昨天说的那版排期，我同意。",
  "user_id": "default_user",
  "timestamp": "2026-10-29 23:40:00"
}
```

`timestamp`（别名 `occurred_at`）支持三种写法：

| 写法 | 示例 | 说明 |
|------|------|------|
| RFC3339 | `2026-10-30T09:30:00+08:00` | 最精确，推荐 |
| 本地日期时间 | `2026-10-30 09:30[:00]` | 按**本地时区**解释 |
| 仅日期 | `2026-10-30` | 按当天 **12:00**（本地）归档 |

不传则用当前时间。解析失败、晚于当前时间 1 天以上、或早于 2000-01-01 均返回 **400**。
时间决定它落在哪一"天"的页里，详见下面的「字典库（原文）」。

### 查询抽取状态

```http
GET /api/memory/extract_status/:conversation_id
```

```json
{ "conversation_id": "...", "status": "completed", "extraction_error": null }
```

`status`：`processing` → `completed` / `failed`。

### 重放

```http
POST /api/memory/replay/:conversation_id      # 重放单条
POST /api/memory/replay_failed?limit=10       # 批量重放失败的
```

---

## 检索

### 语义检索（向量）

```http
POST /api/memory/search/semantic

{
  "query": "Redis 缓存过期时间设置",
  "limit": 5,
  "scope": "user",
  "user_id": "default_user",
  "source_types": ["event", "learning"]
}
```

### 联系人检索

```http
POST /api/memory/search/contacts

{
  "query": "老王",
  "limit": 5,
  "user_id": "default_user",
  "include_archived": false,   // 是否把冷表（已归档）里的也列出来
  "include_virtual": false     // 是否把虚拟角色混进普通联系人结果
}
```

**冷热两级检索**：先查热表（`is_archived = 0`，只保留最近活跃的 50 个），
热表没查够再补查冷表（`is_archived = 1`）——所以人被热表裁剪归档后**依然搜得到**，
只是排在后面。`include_archived=true` 时返回"热 + 冷"全量。

**虚拟角色默认不混入**：`is_virtual = 1` 的行（历史人物、动漫/科幻角色等）
走 `/api/virtual_entities`，本接口默认过滤掉它们；需要一起看时传 `include_virtual: true`。

返回每个联系人的 `contact_id` / **`disambiguation`** / `aliases` / `background`（人设 JSON）/
`relationship_tags` / `related_contacts`（已解析成姓名）以及 **`is_virtual`**。

### 同名联系人：姓名不唯一，靠编号 + 消歧标签区分

`contact_index` 的主键是 UUID，**姓名不唯一**。同一个用户下可以存在两条 `name` 相同的记录：

```json
{ "data": [
  { "contact_id": "68b9b1bf-…", "name": "刘洋", "disambiguation": "前端", "relationship_tags": ["同事"] },
  { "contact_id": "682751b1-…", "name": "刘洋", "disambiguation": "后端", "relationship_tags": ["同事"] }
] }
```

写入去重规则（宁可合并，也不乱建号）：

匹配不是"只比姓名"，而是**姓名 + 消歧标签 + 别名 + 身份线索（role）多字段投票**：

| 新提取到的 | 库里已有 | 结果 |
|-----------|---------|------|
| `刘洋` + 标签`前端` | 同名且标签 `前端` | 同一人，复用原 `contact_id` |
| `刘洋` + 标签`前端` | 同名但还没标签 | 同一人第一次被区分 → 复用并补标签 |
| `刘洋` + 标签`前端` | 同名但标签是 `后端` | **另一个人 → 新建独立编号** |
| `刘洋` + 身份`前端工程师` | 同名，备注 `身份：后端工程师` | **另一个人 → 新建**（同类身份不同取值） |
| `刘洋` + 身份`前端工程师` | 同名，备注 `身份：前端工程师` | 同一人（跨对话靠身份线索认出） |
| `刘洋`（无标签无线索） | 有多个同名 | 归到最近互动的那个，不新建 |
| `前端刘洋`（区分词并进了姓名） | 库里已有 `刘洋` | 剥掉前缀，按 `刘洋` + `前端` 处理 |

身份线索来自抽取时的 `role`（部门/职位/关系，如"前端工程师"），
写入后存在联系人的 `notes` 里（`身份：前端工程师`），
所以**今天说过的部门，下周只说"刘洋"也能对上**。

> 客户端要用 `contact_id` 做引用，不要用姓名；姓名只用于展示，
> 同名时请把 `disambiguation` 一起显示成 `刘洋（前端）`。

### 联系人全息详情

```http
GET /api/contacts/:contact_id/detail?user_id=default_user&event_limit=20&memory_limit=10
```

一次拿到「人设 + 关联联系人 + 关联事件 + 相关记忆原文」，用于人物推理与关系追溯：

```json
{
  "success": true,
  "data": {
    "contact": {
      "contact_id": "…", "name": "老王",
      "aliases": ["王建国"], "is_virtual": false, "is_agent": false,
      "background": "{\"personal_info\":{…}\",\"social_relations\":[…]}",
      "relationship_tags": ["同事"], "activity_score": 0.8
    },
    "related_contacts": [
      { "contact_id": "…", "name": "小李", "relationship_tags": ["同事"] }
    ],
    "events": [
      { "event_id": "…", "title": "架构评审会", "summary": "…",
        "event_time": "2026-11-04T14:00:00Z", "importance": "high", "status": "pending" }
    ],
    "memories": [
      { "conversation_id": "…", "snippet": "老王是我的同事，在研发部做后端…",
        "timestamp": "2026-10-29T…", "matched_keyword": "老王" }
    ],
    "stats": { "event_count": 3, "memory_count": 2, "related_count": 2 }
  }
}
```

| 参数 | 默认 | 说明 |
|------|------|------|
| `user_id` | 配置里的默认用户 | 归属用户 |
| `scope` | `user` | `user` / `agent` |
| `event_limit` | 20 | 最多返回多少事件（上限 100） |
| `memory_limit` | 10 | 最多返回多少条记忆原文（上限 50，扫描上界 500 条对话） |

> 相关记忆按「姓名或任一别名逐字命中」筛选，取最近 500 条对话内有界扫描，
> 不会为了这个查询遍历整个字典库。联系人不存在时返回 **404**。

### 事件检索

```http
POST /api/memory/search/events
{ "query": "项目评审", "limit": 10, "owner_type": "User", "owner_id": "default_user" }
```

### 历史对话检索

```http
POST /api/memory/search/history
{ "query": "上次讨论的预算", "limit": 5, "user_id": "default_user" }
```

### 智能融合检索

```http
POST /api/search/smart
{ "query": "预算", "limit": 5 }
```

---

## 待办

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/todos` | 列表（支持 `status` / `contact_id` / `include_archived` 过滤） |
| POST | `/api/todos` | 新建 |
| GET | `/api/todos/:id` | 详情 |
| PUT | `/api/todos/:id/complete` | 完成（**幂等**，见下） |
| DELETE | `/api/todos/:id` | 删除 |

新建：

```json
{ "title": "整理下季度客户反馈", "deadline": "2026-11-02T23:59:59+08:00", "importance": "high" }
```

`importance`：`high` / `medium` / `low`。

**完成的幂等语义（v0.6.4 起）**：`completed_at` 只在首次完成时写入，重复调用不会覆盖，
只有 `last_updated_at` 会刷新。这样外部重试、心跳或对话抽取反复判定"已完成"都不会让完成时间向后漂移。
待办不存在时仍返回失败（错误语义不变）。

**没有字段级更新接口**：本 API 不提供 `PUT/PATCH /api/todos/:id` 改标题、截止时间或重要性
（这类请求返回 `405`）。调整截止时间走对话链路——抽取出的 `todo_updates` 会调用
`update_todo_deadline`，且**仅对 `pending` 状态生效**；已完成待办需先删除再新建。

待办默认按活跃度做热冷分层，冷待办仍可查询但降一级优先级。

---

## 事件合并确认

相似度落在 **0.75–0.90** 的事件不会自动合并，而是进入待确认队列。

```http
GET /api/events/pending_merges?owner_type=User&owner_id=default_user&limit=20
```

```json
{ "count": 11, "merges": [ { "id": 1, "event_id": "...", "candidate_id": "...", "similarity": 0.8407 } ] }
```

确认：

```http
POST /api/events/pending_merges/:id/resolve

{ "action": "merge" }      // merge | create | dismiss
```

- `merge`：合并到现有事件；
- `create`：作为新事件落库；
- `dismiss`：忽略该候选。

返回 `{"resolved": true}`。

---

## 文件

### 上传

```http
POST /api/files/upload?filename=notes.txt&scope=user&user_id=default_user
Content-Type: application/octet-stream

<二进制内容>
```

```json
{ "success": true, "file_id": "9a443a5b-..." }
```

### 读取

```http
GET /api/files/:file_id?scope=user&user_id=default_user        # 元数据
GET /api/files/:file_id/text?scope=user&user_id=default_user    # 提取的文本
DELETE /api/files/:file_id?scope=user&user_id=default_user      # 删除
```

他人的文件返回 `{"success": false, "error": "文件不存在"}`（HTTP 200，不泄露存在性）。

---

## 心跳

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/heartbeat/status` | 当前心跳状态（间隔、上次执行、跳过次数） |
| GET | `/api/heartbeat/pending_questions` | 待用户确认的问题 |
| POST | `/api/heartbeat/trigger` | 手动触发，`{"maintenance_only": true}` 只做维护不调 LLM |
| POST | `/api/heartbeat/answer` | 回答待确认问题 |

---

## 虚拟联系人

虚拟角色**存放在独立表**（`virtual_character` / `virtual_character_alias`），
不在 `contact_index` 里——角色有原著设定（写死的事实），不参与画像迭代、热表裁剪与同名去重。

```http
GET  /api/virtual_entities                     # 列表（含内置 + 自建）
GET  /api/virtual_entities/:id                 # 详情（含别名、原著设定）
GET  /api/virtual_entities/check/:name         # 该名字是否已存在（按姓名或别名匹配）
POST /api/virtual_entities                     # 新建
PUT  /api/virtual_entities/:id                 # 改名字 / 别名 / 设定
DELETE /api/virtual_entities/:id               # 删除（内置角色不允许删）
```

```json
POST /api/virtual_entities
{
  "name": "雅典娜",
  "source_work": "希腊神话",     // 出自哪部作品 / 哪个体系；同名不同作品可共存
  "category": "myth",            // myth/history/anime/game/novel/other
  "canon": "{\"identity\":\"智慧与战争女神\"}",   // 原著设定（不可改写）
  "persona": "说话冷静、书面语",                    // 扮演用的人设
  "aliases": [{ "name": "帕拉斯·雅典娜", "alias_type": "official" }],
  "user_id": "default_user"
}
```

响应：

```json
{
  "success": true,
  "data": {
    "entity_id": "…", "name": "雅典娜",
    "source_work": "希腊神话", "category": "myth",
    "canon": "{\"identity\":\"智慧与战争女神\"}", "persona": "说话冷静、书面语",
    "is_builtin": false,
    "aliases": [{ "name": "帕拉斯·雅典娜", "source_work": "希腊神话", "alias_type": "official" }],
    "created_at": "2026-10-30T…", "updated_at": "2026-10-30T…"
  }
}
```

> 唯一约束是 `(name, IFNULL(source_work,''), owner_id, owner_type)`：
> 「雅典娜（希腊神话）」和「雅典娜（圣斗士星矢）」可以同时存在。
> 对话里提到角色时，抽取链路不会给他建真人联系人，而是写入
> `event_character_link`（事件 ↔ 角色）。

创建虚拟角色会同时写入一条 `contact_index` 记录并置 `is_virtual = 1`，
因此它和真实联系人共用同一套检索/详情接口，靠 `is_virtual` 区分：

```http
POST /api/virtual_entities
{ "name": "苏晚晴", "aliases": [{ "name": "晚晴", "alias_type": "nickname" }], "user_id": "default_user" }
```

> `user_id` 不填时归属 `default_user`。**必须填对**，否则跨 owner 隔离会让你在
> 自己的联系人列表里查不到这个角色（这是 v0.5.0 修复的一个问题）。

---

## 字典库（原文，按天分页）

字典库保存所有写入的对话原文，**永不删除**。每天一页，页的切分不是自然日 00:00，
而是本地时间 `[storage].day_cutoff_hour`（默认 **04:00**）：00:00–03:59 的对话归**前一天**。
这样"聊到凌晨"的一次会话不会被零点切成两页。

所有接口默认按 `scope=User` + `default_user` 过滤；跨 owner 一律查不到。

### 有哪些"天"

```http
GET /api/dictionary/days?user_id=default_user&limit=30&before=2026-10-30
```

```json
{
  "success": true,
  "data": {
    "owner_type": "User",
    "owner_id": "default_user",
    "count": 2,
    "next_before": "2026-10-28",
    "days": [
      { "day": "2026-10-30", "count": 3, "first_at": "2026-10-29T20:00:00+00:00", "last_at": "2026-10-30T04:00:00+00:00" },
      { "day": "2026-10-28", "count": 1, "first_at": "2026-10-28T02:00:00+00:00", "last_at": "2026-10-28T02:00:00+00:00" }
    ]
  }
}
```

倒序（最新在前）。`before` 传上一页最后一天即可继续往前翻；`next_before` 为 `null` 表示到底了。

### 取某一天

```http
GET /api/dictionary/day/2026-10-30?user_id=default_user&limit=50&offset=0
```

```json
{
  "success": true,
  "data": {
    "day": "2026-10-30",
    "offset": 0, "limit": 50, "count": 2, "has_more": false,
    "conversations": [
      {
        "conversation_id": "…",
        "timestamp": "2026-10-29T20:00:00+00:00",
        "session_day": "2026-10-30",
        "ingested_at": "2026-10-30T00:12:00+00:00",
        "owner_type": "User", "owner_id": "default_user",
        "extracted_status": "completed", "extraction_error": null,
        "chars": 24, "preview": "早上四点整，换页了。"
      }
    ]
  }
}
```

按时间升序；`offset` 是页内偏移，`has_more` 只表示"这一页取满了"，不代表一定还有下一条。

### 取单条原文

```http
GET /api/dictionary/conversation/:conversation_id?user_id=default_user
```

返回完整 `content`（不是预览），带 `timestamp` / `extracted_status` / `chars`。
跨 owner 或不存在都返回 404。

### 自检统计

```http
GET /api/dictionary/stats
```

```json
{
  "success": true,
  "data": {
    "conversations": 42, "days": 5,
    "oldest_at": "2026-10-24T01:00:00+00:00", "newest_at": "2026-10-30T04:00:00+00:00",
    "status_counts": { "completed": 40, "pending": 1, "failed": 1 },
    "cutoff_hour": 4, "utc_offset_minutes": 480
  }
}
```

用于确认"写进去的条数"和"能查到的条数"没有裂缝（`conversations` 与各天 `count` 之和应一致）。

### 当前分页口径

```http
GET /api/dictionary/paging
```

```json
{ "success": true, "data": { "cutoff_hour": 4, "utc_offset_minutes": 480, "rule": "会话日：本地时间 cutoff_hour 之前算前一天…" } }
```

### 联系人事件档案

关系库的事件会归档、合并、失效，字典库里的联系人事件档案是**永久副本**：

```http
GET /api/contacts/:contact_id/event_archive?limit=100
```

```json
{
  "success": true,
  "data": {
    "contact_id": "…", "count": 1,
    "events": [
      { "event_id": "…", "event_time": "2026-10-29T12:00:00+00:00", "summary": "和张总开会讨论预算",
        "importance": "long_term", "user_sentiment": "neutral", "conversation_id": "…" }
    ]
  }
}
```

按事件时间倒序。

---

## 向量直连

```http
GET  /api/vector/:id            # 取单条向量
POST /api/vector/search         # 原始向量检索（不走业务层）
```

---

## 错误约定

| HTTP | 场景 |
|------|------|
| 400 | 请求体字段缺失/类型不符 |
| 404 | 路由或资源不存在（跨命名空间访问通常仍是 200 + `success:false`） |
| 408 | 请求处理超时（`timeout` 中间件） |
| 500 | 内部错误，响应体含错误信息 |
