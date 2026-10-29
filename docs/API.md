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

{ "query": "老王", "limit": 5, "user_id": "default_user" }
```

返回每个联系人的 `aliases` / `background`（人设 JSON）/ `relationship_tags` /
`related_contacts`（已解析成姓名）以及 **`is_virtual`**（是否为智能体扮演的虚拟人物）。

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
| GET | `/api/todos` | 列表 |
| POST | `/api/todos` | 新建 |
| PUT | `/api/todos/:id/complete` | 完成 |
| DELETE | `/api/todos/:id` | 删除 |

新建：

```json
{ "title": "整理下季度客户反馈", "deadline": "2026-11-02T23:59:59+08:00", "importance": "high" }
```

`importance`：`high` / `medium` / `low`。

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

```http
GET  /api/virtual_entities                     # 列表
GET  /api/virtual_entities/:id                 # 详情（含人设）
GET  /api/virtual_entities/check/:name         # 该名字是否已存在
POST /api/virtual_entities                     # 新建（可带 user_id）
PUT  /api/virtual_entities/:id                 # 改名 / 改别名
DELETE /api/virtual_entities/:id               # 删除（仅限 is_virtual=1）
```

创建虚拟角色会同时写入一条 `contact_index` 记录并置 `is_virtual = 1`，
因此它和真实联系人共用同一套检索/详情接口，靠 `is_virtual` 区分：

```http
POST /api/virtual_entities
{ "name": "苏晚晴", "aliases": [{ "name": "晚晴", "alias_type": "nickname" }], "user_id": "default_user" }
```

> `user_id` 不填时归属 `default_user`。**必须填对**，否则跨 owner 隔离会让你在
> 自己的联系人列表里查不到这个角色（这是 v0.5.0 修复的一个问题）。

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
