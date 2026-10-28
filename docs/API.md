# HTTP API

所有接口返回 JSON。`owner` 相关接口需要 `scope` + `user_id`（或 `owner_type` + `owner_id`）定位命名空间，
跨命名空间访问一律返回"不存在"。

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

他人的文件返回 **不存在**（不泄露存在性）。

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
```

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
| 404 | 资源不存在，**或跨命名空间访问** |
| 408 | 请求处理超时（`timeout` 中间件） |
| 500 | 内部错误，响应体含错误信息 |
