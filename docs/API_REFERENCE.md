# ShouXin v0.5.3 API 参考

> **版本**: v0.5.3
> **更新日期**: 2026-07-16
> **变更**: 记忆生命周期管理（auto_invalid_at 自动失效、promote/demote 提升降级、待办 deadline 到期失效）；事件去重三重防护（analyzer 去重检查、对话状态过滤、LLM 重要性传递）

---

## 目录

### 基本信息
- [Base URL](#base-url)
- [通用参数](#通用参数)
- [认证说明](#认证说明)
- [通用响应格式](#通用响应格式)

### 记忆管理 API
- [1. 添加对话记忆](#1-添加对话记忆)
- [2. 搜索联系人](#2-搜索联系人)
- [3. 搜索事件](#3-搜索事件)
- [4. 搜索历史对话](#4-搜索历史对话)
- [5. 语义搜索](#5-语义搜索)
- [6. 获取提取状态](#6-获取提取状态)

### 事件合并管理 API
- [7. 获取待合并事件](#7-获取待合并事件)
- [8. 确认事件合并](#8-确认事件合并)

### 待办事项管理 API
- [9. 创建待办](#9-创建待办)
- [10. 获取待办列表](#10-获取待办列表)
- [11. 获取待办详情](#11-获取待办详情)
- [12. 完成待办](#12-完成待办)
- [13. 删除待办](#13-删除待办)

### 智能搜索 API
- [14. 统一智能搜索](#14-统一智能搜索)

### 心跳模块 API
- [15. 触发心跳扫描](#15-触发心跳扫描)
- [16. 获取待确认问题](#16-获取待确认问题)
- [17. 获取心跳状态](#17-获取心跳状态)
- [18. 回答问题](#18-回答问题)

### 文件管理 API
- [19. 上传文件](#19-上传文件)
- [20. 获取文件元数据](#20-获取文件元数据)
- [21. 获取文件文本内容](#21-获取文件文本内容)
- [22. 删除文件](#22-删除文件)

### 管理 API
- [23. 获取 LLM 状态](#23-获取-llm-状态)
- [24. 切换 LLM 后端](#24-切换-llm-后端)

### 向量搜索 API
- [25. 向量语义搜索](#25-向量语义搜索)
- [26. 获取学习素材详情](#26-获取学习素材详情)

### 虚拟实体管理 API
- [27. 获取虚拟实体列表](#27-获取虚拟实体列表)
- [28. 获取虚拟实体详情](#28-获取虚拟实体详情)
- [29. 添加虚拟实体](#29-添加虚拟实体)
- [30. 更新虚拟实体](#30-更新虚拟实体)
- [31. 删除虚拟实体](#31-删除虚拟实体)
- [32. 检查名称匹配](#32-检查名称匹配)

### 健康检查
- [33. 健康检查](#33-健康检查)

### 附录
- [v0.5.3 重要说明](#v053-重要说明)
- [v0.5.2 重要说明](#v052-重要说明)
- [v0.5.1 重要说明](#v051-重要说明)
- [时间格式说明](#时间格式说明)
- [错误处理](#错误处理)

---

## Base URL

```
http://127.0.0.1:3000
```

- **Content-Type**: `application/json`（文件上传使用 `application/octet-stream`）
- **编码**: UTF-8
- **超时时间**: 可配置，默认 60 秒，最小 30 秒

---

## 通用参数

所有端点均支持以下通用参数，用于数据隔离：

| 参数名 | 类型 | 默认值 | 说明 |
|--------|------|--------|------|
| `scope` | String | `User` | 操作范围，可选值：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | — | 用户标识，用于多用户数据隔离 |

> `scope` 说明：
> - `User`：以用户视角操作数据
> - `Agent`：以 Agent 视角操作数据
> - `Contact`：以联系人视角操作数据
> - `Public`：公共数据，无隔离限制

---

## 认证说明

| API 类别 | 认证要求 |
|----------|----------|
| 公开 API (`/api/`) | 无需认证 |
| 管理 API (`/admin/`) | **必须**提供 `X-Internal-Token` 请求头 |
| 健康检查 (`/health`) | 无需认证 |

> Token 在 `config.toml` 的 `[agent] internal_api_token` 或 `[server] internal_api_token` 中配置。

管理 API 调用示例：

```bash
curl -H "X-Internal-Token: your-token-here" http://127.0.0.1:3000/admin/llm/status
```

---

## 通用响应格式

成功响应：

```json
{
  "success": true,
  "data": { ... }
}
```

失败响应：

```json
{
  "success": false,
  "data": null,
  "error": "错误信息描述"
}
```

---

## 记忆管理 API

### 1. 添加对话记忆

> **用途**: 将用户自然语言对话添加到记忆系统，自动提取联系人、事件和学习内容
>
> **使用场景**: 用户说"明天下午帮我约一下张三"时调用

**端点**: `POST /api/memory/add`

**请求体**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `conversation` | String | ✅ 是 | — | 用户输入的自然语言对话内容 |
| `user_id` | String | ❌ 否 | — | 用户标识，不填则自动生成 |
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |

**请求示例**:

```json
{
  "conversation": "我是张三，明天下午三点要和李四在大会议室开会讨论项目进度",
  "user_id": "user_001",
  "scope": "User"
}
```

**响应**:

```json
{
  "success": true,
  "data": {
    "contacts": [
      {
        "contact_id": "c6ce92b0-0345-45a9-8a99-b2a9bca54102",
        "name": "张三",
        "aliases": [],
        "owner_type": "User",
        "owner_id": "user_001"
      }
    ],
    "events": [
      {
        "event_id": "0c3028ee-ba83-4fda-bf53-af1da41db54c",
        "title": "和李四开会讨论项目进度",
        "event_type": "meeting",
        "description": "明天下午三点要和李四在大会议室开会讨论项目进度",
        "time_range_start": "2026-06-03T15:00:00+08:00",
        "importance": "medium",
        "status": "pending"
      }
    ],
    "learnings": [],
    "conversation_id": "e8f3a1b2-4c5d-6e7f-8a9b-0c1d2e3f4a5b"
  }
}
```

**响应字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `contacts` | Array | 从对话中提取的联系人列表 |
| `events` | Array | 从对话中提取的事件列表 |
| `learnings` | Array | 从对话中提取的学习内容 |
| `conversation_id` | String | 生成的对话唯一标识 |

---

### 2. 搜索联系人

> **用途**: 在已存储的记忆库中搜索匹配的联系人
>
> **使用场景**: 用户问"我和张三有什么约定"时调用

**端点**: `POST /api/memory/search/contacts`

**请求体**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `query` | String | ✅ 是 | — | 搜索关键词，如联系人姓名 |
| `limit` | Number | ❌ 否 | `10` | 返回结果数量 |
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |
| `include_archived` | Boolean | ❌ 否 | `false` | 是否包含已归档联系人（热冷分离） |

**请求示例**:

```json
{
  "query": "张三",
  "limit": 10,
  "scope": "User",
  "user_id": "user_001",
  "include_archived": false
}
```

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "contact_id": "c6ce92b0-0345-45a9-8a99-b2a9bca54102",
      "name": "张三",
      "aliases": ["小张", "张工"],
      "owner_type": "User",
      "owner_id": "user_001"
    }
  ]
}
```

---

### 3. 搜索事件

> **用途**: 搜索符合条件的时间安排和约定。v0.5.0 中事件有了独立的 `title` 字段，建议按标题搜索以获得更精准的结果。
>
> **使用场景**: 用户问"这周有什么安排"时调用

**端点**: `POST /api/memory/search/events`

**请求体**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `event_type` | String | ❌ 否 | — | 事件类型过滤（如 `meeting`、`reminder`） |
| `start_time` | String | ❌ 否 | — | 开始时间（RFC3339 格式） |
| `end_time` | String | ❌ 否 | — | 结束时间（RFC3339 格式） |
| `importance` | String | ❌ 否 | — | 重要性过滤：`low`、`medium`、`high` |
| `status` | String | ❌ 否 | — | 状态过滤：`pending`、`completed` |
| `limit` | Number | ❌ 否 | `10` | 返回结果数量 |
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |
| `include_archived` | Boolean | ❌ 否 | `false` | 是否包含已归档事件 |
| `archived_month` | String | ❌ 否 | — | 按归档月份过滤（如 "2026-06"） |

**请求示例**:

```json
{
  "event_type": "meeting",
  "start_time": "2026-06-01T00:00:00+08:00",
  "end_time": "2026-06-07T23:59:59+08:00",
  "importance": "medium",
  "status": "pending",
  "limit": 10,
  "scope": "User",
  "user_id": "user_001",
  "include_archived": false
}
```

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "event_id": "0c3028ee-ba83-4fda-bf53-af1da41db54c",
      "title": "和李四开会讨论项目进度",
      "event_type": "meeting",
      "description": "明天下午三点要和李四在大会议室开会讨论项目进度",
      "time_range_start": "2026-06-03T15:00:00+08:00",
      "importance": "medium",
      "status": "pending"
    }
  ]
}
```

---

### 4. 搜索历史对话

> **用途**: 按时间范围查询历史对话记录
>
> **使用场景**: 用户问"昨天我和 AI 聊了什么"时调用

**端点**: `POST /api/memory/search/history`

**请求体**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `start_time` | String | ❌ 否 | — | 查询开始时间（RFC3339 格式） |
| `end_time` | String | ❌ 否 | — | 查询结束时间（RFC3339 格式） |
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |

**请求示例**:

```json
{
  "start_time": "2026-06-01T00:00:00+08:00",
  "end_time": "2026-06-02T23:59:59+08:00",
  "scope": "User",
  "user_id": "user_001"
}
```

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "conversation_id": "e8f3a1b2-4c5d-6e7f-8a9b-0c1d2e3f4a5b",
      "content": "[user] 明天下午三点开会\n[assistant] 好的，已为您记录",
      "created_at": "2026-06-02T10:30:00Z"
    }
  ]
}
```

---

### 5. 语义搜索

> **用途**: 在向量数据库中进行语义相似度搜索，按来源类型过滤
>
> **使用场景**: 用户查找相关学习资料或相关内容时调用

**端点**: `POST /api/memory/search/semantic`

**请求体**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `query` | String | ✅ 是 | — | 自然语言搜索查询 |
| `limit` | Number | ❌ 否 | `10` | 返回结果数量 |
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |
| `source_types` | Array | ❌ 否 | — | 来源类型过滤（如 `["learning", "conversation"]`） |
| `conversation_id` | String | ❌ 否 | — | 限定在特定对话内搜索（v0.5.0 新增） |

**请求示例**:

```json
{
  "query": "项目进度讨论",
  "limit": 10,
  "scope": "User",
  "user_id": "user_001",
  "source_types": ["learning", "conversation"],
  "conversation_id": "e8f3a1b2-4c5d-6e7f-8a9b-0c1d2e3f4a5b"
}
```

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "content": "项目进度相关讨论内容...",
      "score": 0.95,
      "source_type": "conversation"
    }
  ]
}
```

---

### 6. 获取提取状态

> **用途**: 查询某次对话记忆的提取处理状态
>
> **使用场景**: 提交对话后轮询提取进度

**端点**: `GET /api/memory/extract_status/{conversation_id}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `conversation_id` | String | ✅ 是 | 对话唯一标识 |

**响应**:

```json
{
  "success": true,
  "data": {
    "status": "completed",
    "conversation_id": "e8f3a1b2-4c5d-6e7f-8a9b-0c1d2e3f4a5b",
    "started_at": "2026-06-05T10:30:00Z",
    "completed_at": "2026-06-05T10:30:01Z",
    "contacts_extracted": 2,
    "events_extracted": 1,
    "learnings_extracted": 0
  }
}
```

**状态说明**:

| 状态值 | 说明 |
|--------|------|
| `pending` | 等待处理 |
| `processing` | 正在提取中 |
| `completed` | 提取完成 |
| `failed` | 提取失败 |

---

## 事件合并管理 API

### 7. 获取待合并事件

> **用途**: 获取指定 Owner 下的待合并事件列表，系统自动识别疑似重复或相关的事件供用户确认
>
> **使用场景**: 心跳检测到可能重复的事件后，前端展示待合并列表时调用

**端点**: `POST /api/events/pending_merges`

**请求体**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `owner_type` | String | ✅ 是 | — | 所有者类型（如 `User`、`Agent`） |
| `owner_id` | String | ✅ 是 | — | 所有者 ID |
| `limit` | Number | ❌ 否 | `20` | 返回结果数量上限 |

**请求示例**:

```json
{
  "owner_type": "User",
  "owner_id": "user_001",
  "limit": 20
}
```

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "source_event_id": "evt-001",
      "target_event_id": "evt-002",
      "source_event_summary": "和张三讨论方案",
      "target_event_summary": "与张三开会",
      "similarity_score": 0.85,
      "status": "pending",
      "created_at": "2026-06-05T10:30:00Z"
    }
  ]
}
```

**响应字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | Number | 合并候选记录 ID |
| `source_event_id` | String | 来源事件 ID |
| `target_event_id` | String (nullable) | 目标事件 ID |
| `source_event_summary` | String | 来源事件摘要 |
| `target_event_summary` | String (nullable) | 目标事件摘要 |
| `similarity_score` | Number | 相似度分数（0~1） |
| `status` | String | 状态：`pending`、`merged`、`created`、`dismissed` |
| `created_at` | String | 创建时间 |

---

### 8. 确认事件合并

> **用途**: 用户对系统建议的合并候选做出确认操作（合并为已有事件，或创建为独立新事件）
>
> **使用场景**: 用户在待合并列表中选择合并或分离操作时调用

**端点**: `POST /api/events/confirm_merge`

**请求体**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `merge_id` | Number | ✅ 是 | 合并候选记录 ID（对应 PendingMergeItem.id） |
| `action` | String | ✅ 是 | 操作类型：`merge`（合并）或 `create`（创建为新事件） |
| `source_event_summary` | String | ❌ 否 | `action=create` 时用于生成新事件的摘要描述 |

**请求示例（合并操作）**:

```json
{
  "merge_id": 1,
  "action": "merge"
}
```

**请求示例（创建为新事件）**:

```json
{
  "merge_id": 1,
  "action": "create",
  "source_event_summary": "和张三单独讨论技术方案"
}
```

**响应**:

```json
{
  "success": true,
  "data": {
    "success": true,
    "message": "事件已合并",
    "event_id": "evt-001"
  }
}
```

---

## 待办事项管理 API

### 9. 创建待办

> **用途**: 创建一个新的待办事项。v0.5.1 新增归档与提醒机制：写入时若 pending 超过 50 条，自动归档超过 30 天的旧待办；若 pending 超过 30 条，自动生成提醒。
>
> **使用场景**: 系统自动从对话中提取任务时调用，或用户直接创建待办

**端点**: `POST /api/todos`

**请求体**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `title` | String | ✅ 是 | — | 待办事项标题 |
| `description` | String | ❌ 否 | — | 详细描述 |
| `deadline` | String | ❌ 否 | — | 截止时间（RFC3339 格式） |
| `importance` | String | ❌ 否 | `medium` | 重要性：`high`、`medium`、`low` |
| `todo_type` | String | ❌ 否 | `task` | 待办类型：`task`、`reminder`、`follow_up` |
| `contact_ids` | Array | ❌ 否 | — | 关联的联系人 ID 列表 |
| `todo_id` | String | ❌ 否 | — | 自定义待办 ID，不填则自动生成 UUID |
| `source_event_id` | String | ❌ 否 | — | 来源事件 ID |
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID |

**请求示例**:

```json
{
  "title": "和张三开会讨论项目进度",
  "description": "在大会议室，带上项目文档",
  "deadline": "2026-06-03T15:00:00+08:00",
  "importance": "high",
  "todo_type": "task",
  "contact_ids": ["c6ce92b0-0345-45a9-8a99-b2a9bca54102"],
  "scope": "User",
  "user_id": "user_001"
}
```

**响应**:

```json
{
  "success": true,
  "data": {
    "todo_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "title": "和张三开会讨论项目进度",
    "deadline": "2026-06-03T15:00:00+08:00",
    "status": "pending",
    "importance": "high",
    "todo_type": "task",
    "is_archived": false,
    "archived_at": null,
    "last_updated_at": "2026-06-05T10:30:00Z"
  }
}
```

> **v0.5.1 说明**: 写入时触发归档检查 — 若当前 pending 待办超过 **50 条**，系统自动将超过 **30 天**未更新的旧待办标记为已归档（`is_archived=true`）。若 pending 超过 **30 条**，系统自动生成一条提醒类型待办。

> **v0.5.0 说明**: 相同标题但不同截止时间的待办现在允许并存。唯一索引为 `(title, owner_id, owner_type, deadline)`，只有四者完全相同时才视为重复。

**新增字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `todo_type` | String | 待办类型：`task`（任务）、`reminder`（提醒）、`follow_up`（跟进） |
| `is_archived` | Boolean | 是否已归档 |
| `archived_at` | String | 归档时间（未归档时为 null） |
| `last_updated_at` | String | 最后更新时间 |

---

### 10. 获取待办列表

> **用途**: 获取符合过滤条件的待办事项列表
>
> **使用场景**: 用户查看待办列表时调用

**端点**: `GET /api/todos`

**查询参数**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `status` | String | ❌ 否 | — | 状态过滤：`pending`、`completed` |
| `contact_id` | String | ❌ 否 | — | 按关联联系人 ID 过滤 |
| `todo_type` | String | ❌ 否 | — | 按待办类型过滤：`task`、`reminder`、`follow_up` |
| `is_archived` | Boolean | ❌ 否 | — | 是否只显示已归档（默认只显示未归档） |
| `include_archived` | Boolean | ❌ 否 | `false` | 是否包含已归档待办 |
| `limit` | Number | ❌ 否 | `100` | 返回结果数量 |
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |

**请求示例**:

```
GET /api/todos?status=pending&todo_type=task&include_archived=false&limit=10&scope=User&user_id=user_001
```

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "todo_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
      "title": "和张三开会讨论项目进度",
      "status": "pending",
      "deadline": "2026-06-03T15:00:00+08:00",
      "todo_type": "task",
      "is_archived": false,
      "archived_at": null,
      "last_updated_at": "2026-06-05T10:30:00Z"
    }
  ]
}
```

---

### 11. 获取待办详情

> **用途**: 根据 ID 获取单个待办事项的完整信息
>
> **使用场景**: 用户点击某个待办查看详情时调用

**端点**: `GET /api/todos/{id}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `id` | String | ✅ 是 | 待办事项 UUID |

**查询参数**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |

**请求示例**:

```
GET /api/todos/a1b2c3d4-e5f6-7890-abcd-ef1234567890?scope=User&user_id=user_001
```

**响应**:

```json
{
  "success": true,
  "data": {
    "todo_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "title": "和张三开会讨论项目进度",
    "description": "在大会议室，带上项目文档",
    "deadline": "2026-06-03T15:00:00+08:00",
    "status": "pending",
    "importance": "high",
    "todo_type": "task",
    "is_archived": false,
    "archived_at": null,
    "last_updated_at": "2026-06-05T10:30:00Z",
    "contact_ids": ["c6ce92b0-0345-45a9-8a99-b2a9bca54102"],
    "source_event_id": null,
    "created_at": "2026-06-02T10:30:00Z",
    "completed_at": null
  }
}
```

---

### 12. 完成待办

> **用途**: 将指定待办标记为已完成
>
> **使用场景**: 用户完成某个待办后调用

**端点**: `PUT /api/todos/{id}/complete`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `id` | String | ✅ 是 | 待办事项 UUID |

**查询参数**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |

**请求示例**:

```
PUT /api/todos/a1b2c3d4-e5f6-7890-abcd-ef1234567890/complete?scope=User&user_id=user_001
```

**响应**:

```json
{
  "success": true,
  "data": {
    "todo_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "status": "completed",
    "completed_at": "2026-06-05T11:00:00Z",
    "last_updated_at": "2026-06-05T11:00:00Z"
  }
}
```

---

### 13. 删除待办

> **用途**: 删除指定的待办事项
>
> **使用场景**: 用户不再需要某个待办时调用

**端点**: `DELETE /api/todos/{id}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `id` | String | ✅ 是 | 待办事项 UUID |

**查询参数**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |

**请求示例**:

```
DELETE /api/todos/a1b2c3d4-e5f6-7890-abcd-ef1234567890?scope=User&user_id=user_001
```

**响应**:

```json
{
  "success": true,
  "data": {}
}
```

---

## 智能搜索 API

### 14. 统一智能搜索

> **用途**: 在多个数据源（联系人、事件、历史、待办）中执行统一搜索，一次查询返回所有匹配结果
>
> **使用场景**: 用户使用全局搜索功能时调用

**端点**: `POST /api/search/smart`

**请求体**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `query` | String | ✅ 是 | — | 搜索查询内容 |
| `limit` | Number | ❌ 否 | `10` | 每个类别返回的结果数量 |
| `types` | Array | ❌ 否 | `["contacts", "events", "history", "todos"]` | 要搜索的数据类型 |
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |

**请求示例**:

```json
{
  "query": "张三",
  "limit": 10,
  "types": ["contacts", "events", "todos"],
  "scope": "User",
  "user_id": "user_001"
}
```

**响应**:

```json
{
  "success": true,
  "data": {
    "contacts": [
      {
        "contact_id": "c6ce92b0-0345-45a9-8a99-b2a9bca54102",
        "name": "张三",
        "aliases": [],
        "owner_type": "User",
        "owner_id": "user_001"
      }
    ],
    "events": [
      {
        "event_id": "0c3028ee-ba83-4fda-bf53-af1da41db54c",
        "title": "和李四开会讨论项目进度",
        "event_type": "meeting",
        "time_range_start": "2026-06-03T15:00:00+08:00",
        "importance": "medium",
        "status": "pending"
      }
    ],
    "history": [],
    "todos": [
      {
        "todo_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        "title": "和张三开会讨论项目进度",
        "status": "pending",
        "deadline": "2026-06-03T15:00:00+08:00",
        "is_archived": false
      }
    ]
  }
}
```

---

## 心跳模块 API

### 15. 触发心跳扫描

> **用途**: 手动触发一次心跳分析，扫描并处理待确认的问题。v0.5.1 心跳报告新增事件合并、联系人合并、歧义检测、档案更新等统计字段。
>
> **使用场景**: 管理员手动触发心跳或定时任务触发时调用

**端点**: `POST /api/heartbeat/trigger`

**请求体**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `maintenance_only` | Boolean | ❌ 否 | `false` | 仅执行维护心跳（不调用 LLM），用于快速执行 SQL 维护任务 |

**请求示例**:

```json
{
  "maintenance_only": false
}
```

**响应**:

```json
{
  "success": true,
  "data": {
    "report": {
      "started_at": "2026-06-05T10:30:00Z",
      "completed_at": "2026-06-05T10:30:01Z",
      "actions_generated": 2,
      "questions_created": 1,
      "contacts_updated": 1,
      "events_cleaned": 0,
      "events_merged": 1,
      "contacts_merged": 0,
      "ambiguities_detected": 1,
      "profiles_updated": 2,
      "maintenance_only": false,
      "duration_ms": 150
    }
  }
}
```

**HeartbeatReport 字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `started_at` | String | 扫描开始时间 |
| `completed_at` | String | 扫描完成时间 |
| `actions_generated` | Number | 生成的行动项数量 |
| `questions_created` | Number | 创建的待确认问题数量 |
| `contacts_updated` | Number | 更新的联系人数量 |
| `events_cleaned` | Number | 清理的过期事件数量 |
| `events_merged` | Number | (v0.5.1 新增) 合并的事件数量 |
| `contacts_merged` | Number | (v0.5.1 新增) 合并的联系人数量 |
| `ambiguities_detected` | Number | (v0.5.1 新增) 检测到的歧义数量 |
| `profiles_updated` | Number | (v0.5.1 新增) 更新的用户档案数量 |
| `maintenance_only` | Boolean | (v0.5.1 新增) 是否仅执行维护操作 |
| `duration_ms` | Number | 扫描耗时（毫秒） |

---

### 16. 获取待确认问题

> **用途**: 获取需要用户确认的待处理问题列表
>
> **使用场景**: 前端显示待确认问题列表时调用

**端点**: `GET /api/heartbeat/pending_questions`

**请求示例**:

```
GET /api/heartbeat/pending_questions
```

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "question_id": "q1a2b3c4-d5e6-7890-abcd-ef1234567890",
      "question": "张三的会议是否已经结束？",
      "created_at": "2026-06-05T10:30:00Z"
    }
  ]
}
```

---

### 17. 获取心跳状态

> **用途**: 查看心跳系统的当前运行状态。v0.5.1 新增最近心跳和内存写入时间、下次心跳倒计时等字段。
>
> **使用场景**: 前端显示心跳系统状态或监控时调用

**端点**: `GET /api/heartbeat/status`

**请求示例**:

```
GET /api/heartbeat/status
```

**响应**:

```json
{
  "success": true,
  "data": {
    "is_running": true,
    "consecutive_skips": 0,
    "pending_questions_count": 1,
    "last_heartbeat": "2026-06-05T10:30:01Z",
    "last_memory_write": "2026-06-05T10:25:00Z",
    "next_heartbeat_in_secs": 295
  }
}
```

**响应字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `is_running` | Boolean | 心跳是否正在运行 |
| `consecutive_skips` | Number | 连续跳过次数 |
| `pending_questions_count` | Number | 待确认问题数量 |
| `last_heartbeat` | String | (v0.5.1 新增) 最近一次心跳执行时间 |
| `last_memory_write` | String | (v0.5.1 新增) 最近一次记忆写入时间 |
| `next_heartbeat_in_secs` | Number | (v0.5.1 新增) 距离下次心跳触发的剩余秒数 |

---

### 18. 回答问题

> **用途**: 用户回答心跳系统提出的待确认问题
>
> **使用场景**: 用户回答系统提出的确认问题时调用

**端点**: `POST /api/heartbeat/answer`

**请求体**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `question_id` | String | ✅ 是 | 问题 ID |
| `answer` | String | ✅ 是 | 用户回答内容 |

**请求示例**:

```json
{
  "question_id": "q1a2b3c4-d5e6-7890-abcd-ef1234567890",
  "answer": "是的，已经结束了"
}
```

**响应**:

```json
{
  "success": true,
  "data": {}
}
```

---

## 文件管理 API

### 19. 上传文件

> **用途**: 将文件上传到系统中存储。v0.5.1 采用异步处理模式：元数据立即可用，文本提取在后台异步进行。支持 MD5 去重，相同内容的文件不会重复存储。
>
> **使用场景**: 用户上传文档、代码等文件时调用

**端点**: `POST /api/files/upload`

**Content-Type**: `application/octet-stream`（文件原始字节流）

**查询参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `filename` | String | ✅ 是 | 文件名（含扩展名） |
| `scope` | String | ❌ 否 | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | 用户标识 |
| `target_type` | String | ❌ 否 | 关联目标类型 |
| `target_id` | String | ❌ 否 | 关联目标 ID |

**请求体**: 文件原始字节流

**支持的文件类型**:

| 类别 | 扩展名 | 说明 |
|------|--------|------|
| 纯文本 | `.txt`、`.md` | 文本文件 |
| 代码 | `.py`、`.js`、`.ts`、`.rs`、`.go`、`.java` 等 | 源代码文件 |
| 文档 | `.pdf` | PDF 文档 |
| 文档 | `.doc`、`.docx` | Word 文档 |
| 表格 | `.xls`、`.xlsx` | Excel 表格 |
| 数据 | `.csv` | CSV 数据文件 |

**限制**:
- 单个文件最大 **20 MB**
- 超过限制返回 `413 Request Entity Too Large`

**MD5 去重**: 系统自动计算文件的 MD5 哈希值，相同 MD5 的文件仅存储一份，后续上传返回已有 file_id。

**异步处理**: 上传成功后元数据立即可用；文本提取（如 PDF 解析、代码语法高亮预处理）在后台异步进行，可通过文件元数据接口查询处理状态。

**请求示例**:

```
POST /api/files/upload?filename=report.pdf&scope=User&user_id=user_001&target_type=event&target_id=evt-001
Content-Type: application/octet-stream

<file raw bytes>
```

**响应**:

```json
{
  "success": true,
  "data": {
    "file_id": "f1a2b3c4-d5e6-7890-abcd-ef1234567890",
    "filename": "report.pdf",
    "md5": "d41d8cd98f00b204e9800998ecf8427e",
    "size": 102400,
    "file_type": "pdf",
    "processing_status": "metadata_ready",
    "is_duplicate": false,
    "uploaded_at": "2026-06-05T10:30:00Z"
  }
}
```

**响应字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `file_id` | String | 文件唯一标识 |
| `filename` | String | 文件名 |
| `md5` | String | (v0.5.1 新增) 文件 MD5 哈希值 |
| `size` | Number | 文件大小（字节） |
| `file_type` | String | (v0.5.1 新增) 文件类型 |
| `processing_status` | String | (v0.5.1 新增) 处理状态：`metadata_ready`、`extracting`、`ready`、`failed` |
| `is_duplicate` | Boolean | (v0.5.1 新增) 是否为重复文件（MD5 匹配） |
| `uploaded_at` | String | (v0.5.1 新增) 上传时间 |

---

### 20. 获取文件元数据

> **用途**: 获取文件的元数据信息。v0.5.1 新增 MD5、文件类型和处理状态字段。
>
> **使用场景**: 查看文件基本信息时调用

**端点**: `GET /api/files/{file_id}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `file_id` | String | ✅ 是 | 文件 UUID |

**请求示例**:

```
GET /api/files/f1a2b3c4-d5e6-7890-abcd-ef1234567890
```

**响应**:

```json
{
  "success": true,
  "data": {
    "file_id": "f1a2b3c4-d5e6-7890-abcd-ef1234567890",
    "filename": "report.pdf",
    "md5": "d41d8cd98f00b204e9800998ecf8427e",
    "size": 102400,
    "file_type": "pdf",
    "processing_status": "ready",
    "uploaded_at": "2026-06-05T10:30:00Z"
  }
}
```

---

### 21. 获取文件文本内容

> **用途**: 获取文件的文本内容（适用于文本类文件）。若文件仍在异步提取中，返回当前可用内容。
>
> **使用场景**: 需要读取文件内容时调用

**端点**: `GET /api/files/{file_id}/text`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `file_id` | String | ✅ 是 | 文件 UUID |

**请求示例**:

```
GET /api/files/f1a2b3c4-d5e6-7890-abcd-ef1234567890/text
```

**响应**:

```json
{
  "success": true,
  "data": {
    "content": "这是文件的文本内容...",
    "processing_status": "ready",
    "extracted_at": "2026-06-05T10:30:02Z"
  }
}
```

---

### 22. 删除文件

> **用途**: 删除指定的文件
>
> **使用场景**: 用户不再需要某个文件时调用

**端点**: `DELETE /api/files/{file_id}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `file_id` | String | ✅ 是 | 文件 UUID |

**请求示例**:

```
DELETE /api/files/f1a2b3c4-d5e6-7890-abcd-ef1234567890
```

**响应**:

```json
{
  "success": true,
  "data": {}
}
```

---

## 管理 API

### 23. 获取 LLM 状态

> **用途**: 获取当前 LLM 主备模型的状态、健康情况和请求统计
>
> **使用场景**: 监控面板显示 LLM 服务状态

**端点**: `GET /admin/llm/status`

**认证**: **必须**提供 `X-Internal-Token` 请求头

**请求头**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `X-Internal-Token` | String | ✅ 是 | 管理 API 认证 Token |

**响应**:

```json
{
  "success": true,
  "data": {
    "active_source": "gguf",
    "is_primary": true,
    "total_requests": 1234,
    "total_failures": 5,
    "primary_healthy": true,
    "secondary_healthy": true,
    "auto_switch_enabled": true
  }
}
```

**响应字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `active_source` | String | 当前激活的模型源 |
| `is_primary` | Boolean | 当前是否使用主模型 |
| `total_requests` | Number | 累计请求数 |
| `total_failures` | Number | 累计失败数 |
| `primary_healthy` | Boolean | 主模型健康状态 |
| `secondary_healthy` | Boolean | 备模型健康状态 |
| `auto_switch_enabled` | Boolean | 自动切换是否启用 |

---

### 24. 切换 LLM 后端

> **用途**: 手动切换主备模型后端
>
> **使用场景**: 运维人员手动切换模型，或在主模型故障时切换

**端点**: `POST /admin/llm/switch`

**认证**: **必须**提供 `X-Internal-Token` 请求头

**请求头**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `X-Internal-Token` | String | ✅ 是 | 管理 API 认证 Token |

**请求体**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `target` | String | ✅ 是 | 目标后端：`primary` 或 `secondary` |

**请求示例**:

```json
{
  "target": "secondary"
}
```

**响应**:

```json
{
  "success": true,
  "data": {
    "switched_to": "secondary"
  }
}
```

**错误响应**:

| 状态码 | 条件 | 响应 |
|--------|------|------|
| 400 | 无效的 target 值 | `{"success": false, "error": "invalid target: xxx"}` |
| 409 | 未配置备模型 | `{"success": false, "error": "secondary not configured"}` |
| 401 | Token 缺失或无效 | `{"success": false, "error": "Missing or invalid X-Internal-Token"}` |

---

## 向量搜索 API

### 25. 向量语义搜索

> **用途**: 在向量数据库中进行语义相似度搜索
>
> **使用场景**: 用户查找相关内容时调用

**端点**: `POST /api/vector/search`

**请求体**: 与 [语义搜索](#5-语义搜索) 接口参数一致

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `query` | String | ✅ 是 | — | 自然语言搜索查询 |
| `limit` | Number | ❌ 否 | `10` | 返回结果数量 |
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |
| `source_types` | Array | ❌ 否 | — | 来源类型过滤 |
| `conversation_id` | String | ❌ 否 | — | 限定对话 ID（v0.5.0 新增） |

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "content": "匹配的内容片段...",
      "score": 0.95
    }
  ]
}
```

---

### 26. 获取学习素材详情

> **用途**: 根据 ID 获取单个学习素材的完整信息
>
> **使用场景**: 用户点击某个学习资料查看详情时调用

**端点**: `GET /api/vector/{id}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `id` | String | ✅ 是 | 学习素材 ID |

**请求示例**:

```
GET /api/vector/material_001
```

**响应**:

```json
{
  "success": true,
  "data": {
    "id": "material_001",
    "content": "学习素材的完整内容...",
    "source_type": "learning"
  }
}
```

---

## 虚拟实体管理 API

> **概述**: 虚拟实体用于管理系统中的非真实实体——如虚构角色、文学作品人物、历史人物等。每个虚拟实体包含名称和一组别名（含来源作品和别名类型），可用于对话记忆中的名称识别与匹配。

### 27. 获取虚拟实体列表

> **用途**: 获取系统中所有已注册的虚拟实体列表
>
> **使用场景**: 前端展示虚拟实体管理列表时调用

**端点**: `GET /api/virtual_entities`

**查询参数**:

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `scope` | String | ❌ 否 | `User` | 操作范围：`User`、`Agent`、`Contact`、`Public` |
| `user_id` | String | ❌ 否 | — | 用户 ID 过滤 |

**请求示例**:

```
GET /api/virtual_entities?scope=User&user_id=user_001
```

**响应**:

```json
{
  "success": true,
  "data": [
    {
      "entity_id": "ve_a1b2c3d4-e5f6-7890-abcd-ef1234567890",
      "name": "福尔摩斯",
      "aliases": [
        {
          "name": "Sherlock Holmes",
          "source_work": "福尔摩斯探案集",
          "alias_type": "英文名"
        },
        {
          "name": "Holmes",
          "source_work": "福尔摩斯探案集",
          "alias_type": "姓氏"
        }
      ],
      "created_at": "2026-06-05T10:00:00Z",
      "updated_at": "2026-06-05T10:00:00Z"
    }
  ]
}
```

---

### 28. 获取虚拟实体详情

> **用途**: 根据 ID 获取单个虚拟实体的完整信息
>
> **使用场景**: 查看某个虚拟实体的所有别名时调用

**端点**: `GET /api/virtual_entities/{id}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `id` | String | ✅ 是 | 虚拟实体 UUID |

**请求示例**:

```
GET /api/virtual_entities/ve_a1b2c3d4-e5f6-7890-abcd-ef1234567890
```

**响应**:

```json
{
  "success": true,
  "data": {
    "entity_id": "ve_a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "name": "福尔摩斯",
    "aliases": [
      {
        "name": "Sherlock Holmes",
        "source_work": "福尔摩斯探案集",
        "alias_type": "英文名"
      }
    ],
    "created_at": "2026-06-05T10:00:00Z",
    "updated_at": "2026-06-05T10:00:00Z"
  }
}
```

---

### 29. 添加虚拟实体

> **用途**: 添加一个新的虚拟实体，可附带多个别名
>
> **使用场景**: 用户注册新的虚拟角色时调用

**端点**: `POST /api/virtual_entities`

**请求体**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `name` | String | ✅ 是 | 虚拟实体名称 |
| `aliases` | Array | ❌ 否 | 别名列表 |

**aliases 子对象**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `name` | String | ✅ 是 | 别名名称 |
| `source_work` | String | ❌ 否 | 来源作品名称 |
| `alias_type` | String | ❌ 否 | 别名类型（如：英文名、昵称、曾用名） |

**请求示例**:

```json
{
  "name": "福尔摩斯",
  "aliases": [
    {
      "name": "Sherlock Holmes",
      "source_work": "福尔摩斯探案集",
      "alias_type": "英文名"
    },
    {
      "name": "Holmes",
      "source_work": "福尔摩斯探案集",
      "alias_type": "姓氏"
    }
  ]
}
```

**响应**:

```json
{
  "success": true,
  "data": {
    "entity_id": "ve_a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "name": "福尔摩斯",
    "aliases": [
      {
        "name": "Sherlock Holmes",
        "source_work": "福尔摩斯探案集",
        "alias_type": "英文名"
      },
      {
        "name": "Holmes",
        "source_work": "福尔摩斯探案集",
        "alias_type": "姓氏"
      }
    ],
    "created_at": "2026-06-05T10:00:00Z"
  }
}
```

---

### 30. 更新虚拟实体

> **用途**: 更新指定虚拟实体的名称和别名（全量替换）
>
> **使用场景**: 修改虚拟实体信息时调用

**端点**: `PUT /api/virtual_entities/{id}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `id` | String | ✅ 是 | 虚拟实体 UUID |

**请求体**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `name` | String | ✅ 是 | 虚拟实体名称 |
| `aliases` | Array | ❌ 否 | 别名列表（全量替换现有别名） |

**aliases 子对象**: 同 [添加虚拟实体](#29-添加虚拟实体)

**请求示例**:

```json
{
  "name": "夏洛克·福尔摩斯",
  "aliases": [
    {
      "name": "Sherlock Holmes",
      "source_work": "福尔摩斯探案集",
      "alias_type": "英文名"
    }
  ]
}
```

**响应**:

```json
{
  "success": true,
  "data": {
    "entity_id": "ve_a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "name": "夏洛克·福尔摩斯",
    "aliases": [
      {
        "name": "Sherlock Holmes",
        "source_work": "福尔摩斯探案集",
        "alias_type": "英文名"
      }
    ],
    "updated_at": "2026-06-05T11:00:00Z"
  }
}
```

---

### 31. 删除虚拟实体

> **用途**: 删除指定的虚拟实体
>
> **使用场景**: 用户不再需要某个虚拟实体时调用

**端点**: `DELETE /api/virtual_entities/{id}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `id` | String | ✅ 是 | 虚拟实体 UUID |

**请求示例**:

```
DELETE /api/virtual_entities/ve_a1b2c3d4-e5f6-7890-abcd-ef1234567890
```

**响应**:

```json
{
  "success": true,
  "data": {}
}
```

---

### 32. 检查名称匹配

> **用途**: 检查给定名称是否匹配某个已注册的虚拟实体（通过实体名或别名进行精确/模糊匹配）
>
> **使用场景**: 对话提取中检测到人名时，快速判断是否为虚拟实体

**端点**: `GET /api/virtual_entities/check/{name}`

**路径参数**:

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `name` | String | ✅ 是 | 待检查的名称 |

**请求示例**:

```
GET /api/virtual_entities/check/福尔摩斯
```

**响应（匹配成功）**:

```json
{
  "success": true,
  "data": {
    "matched": true,
    "entity": {
      "entity_id": "ve_a1b2c3d4-e5f6-7890-abcd-ef1234567890",
      "name": "福尔摩斯",
      "matched_alias": null
    }
  }
}
```

**响应（通过别名匹配成功）**:

```json
{
  "success": true,
  "data": {
    "matched": true,
    "entity": {
      "entity_id": "ve_a1b2c3d4-e5f6-7890-abcd-ef1234567890",
      "name": "福尔摩斯",
      "matched_alias": {
        "name": "Sherlock Holmes",
        "source_work": "福尔摩斯探案集",
        "alias_type": "英文名"
      }
    }
  }
}
```

**响应（未匹配）**:

```json
{
  "success": true,
  "data": {
    "matched": false,
    "entity": null
  }
}
```

---

## 健康检查

### 33. 健康检查

> **用途**: 检查服务是否正常运行
>
> **使用场景**: 负载均衡健康探测、监控系统巡检

**端点**: `GET /health`

**请求示例**:

```
GET /health
```

**响应**:

```json
{
  "status": "ok",
  "time": "2026-06-05T10:30:00+08:00"
}
```

---

## v0.5.3 重要说明

### v0.5.3 变更 (2026-07-16)

#### 记忆生命周期管理

- **事件自动失效**：事件创建时根据 `importance_score` 自动计算 `auto_invalid_at` 字段
  - `> 0.95` (critical): 永不失效
  - `> 0.7` (长期记忆): 30天后失效
  - `0.4 - 0.7` (短期记忆): 7天后失效
  - `<= 0.4` (低价值): 3天后失效
- **记忆提升/降级**：心跳维护阶段自动调整 `importance_score`
  - 短期记忆被3+个近期事件共享 contact_ids → 提升为长期记忆 (0.75)
  - 长期记忆60天无新引用 → 降级为短期记忆 (0.65)
- **待办 deadline 到期失效**：deadline 已过期的 pending 待办自动标记为 `auto_invalid`
- **过期事件归档**：心跳维护阶段将到期事件标记为 `is_archived=1, is_auto_invalid=1`

#### 事件去重三重防护

- **Layer 1 (background_queue)**：写入前调用 `search_events_by_summary` 查重
- **Layer 2 (analyzer)**：心跳路径 insert 前调用 `search_events_by_summary` 查重
- **Layer 3 (对话状态过滤)**：`get_conversations_since` 只返回 status=pending/failed 的对话，跳过已处理对话
- **DB 约束**：`idx_unique_event_summary` 唯一索引 + `INSERT OR IGNORE` 兜底

#### LLM 重要性传递

- 心跳路径从 LLM 提取结果获取 `importance` 字段（0.0-1.0），而非硬编码 0.5
- LLM prompt 新增 importance 字段说明（>=0.9 关键/0.7-0.9 重要/0.4-0.7 普通/<0.4 低价值）

#### 心跳维护阶段执行顺序

```
1. cleanup_expired_pending_merges     # 清理过期合并请求
2. promote_demote_events              # 记忆提升/降级（必须在cleanup之前）
3. cleanup_expired_events             # 过期事件归档（双路径）
4. cleanup_low_importance_events      # 低重要性事件删除
5. cleanup_orphaned_vector_memories   # 孤立向量记忆清理
6. archive_stale_contacts             # 联系人归档
7. invalidate_expired_todos           # 待办deadline到期失效
8. cleanup_todos                      # 待办归档
9. Todo hot/cold management           # 先归档(>50)后提醒(>30)
```

---

## v0.5.2 重要说明

### v0.5.2 变更 (2026-07-15)

- **POST /api/heartbeat/trigger**: 新增 `maintenance_only` 参数，支持仅执行维护心跳（不调用 LLM）
- **POST /api/memory/search/contacts**: `include_archived` 参数稳定化，支持热冷分离查询
- **POST /api/memory/search/events**: 新增 `include_archived` 和 `archived_month` 参数
- **GET /api/todos**: 新增 `include_archived` 参数
- **心跳稳定性**: 修复 executing 标志 panic 后卡死的问题，引入 RAII guard
- **联系人匹配**: 使用 `find_contact_by_any_name` 按 name OR aliases 匹配，避免创建重复联系人
- **数据保护**: upsert 时保留已有业务字段（background/notes 等），不被空值覆盖

---

## v0.5.1 重要说明

### 新增功能

| 编号 | 功能 | 说明 |
|------|------|------|
| — | 虚拟实体管理 API | 新增虚拟实体的增删改查、别名管理和名称匹配检查功能（6 个端点） |
| — | 事件合并管理 API | 新增待合并事件查询和合并确认功能（2 个端点） |
| — | 心跳报告扩展 | HeartbeatReport 新增 `events_merged`、`contacts_merged`、`ambiguities_detected`、`profiles_updated`、`maintenance_only` 字段 |
| — | 心跳状态扩展 | Status 响应新增 `last_heartbeat`、`last_memory_write`、`next_heartbeat_in_secs` 字段 |
| — | 待办归档机制 | 写入时若 pending > 50，自动归档超过 30 天的旧待办 |
| — | 待办提醒生成 | 写入时若 pending > 30，自动生成提醒类型待办 |
| — | 待办新字段 | 新增 `archived_at`、`todo_type`、`is_archived`、`last_updated_at` 字段 |
| — | 文件异步处理 | 上传后元数据立即可用，文本提取在后台异步进行 |
| — | 文件 MD5 去重 | 基于 MD5 哈希值自动去重，相同内容文件不重复存储 |
| — | 文件类型扩展 | 支持 `txt`、`md`、代码文件、`pdf`、`doc`、`xls`、`csv`；最大 20MB |
| — | Scope 扩展 | `scope` 参数新增 `Contact` 和 `Public` 两个取值 |

### 问题修复

| 编号 | 问题 | 说明 |
|------|------|------|
| **P2-1** | Todo 去重 | 相同标题不同截止时间的待办现在允许并存。唯一索引从 `(title, owner_id, owner_type)` 扩展为 `(title, owner_id, owner_type, deadline)` |
| **P1-2** | 事件提取 | 事件现在拥有独立的 `title` 字段，不再使用完整对话文本作为标题。搜索事件时建议按 `title` 查找以获得更精准的结果 |
| **P1-1** | 联系人提取 | 增强覆盖范围：扩展头衔词库（总监、工程师、组长、经理等）、支持复姓识别（欧阳、诸葛等）、支持昵称检测（小X） |
| **P0-1** | 长对话处理 | 输入截断和工作线程健壮性增强，防止长对话导致服务崩溃 |
| **P0-2** | 回退提取 | 新增基于正则的回退提取机制，确保在 LLM 输出不理想时仍能提取联系人和事件 |

### API 变更摘要

| 变更 | 说明 |
|------|------|
| 待办 `importance` 取值变更 | 从 `short_term` / `long_term` / `critical` 变更为 `high` / `medium` / `low` |
| 待办 `todo_id` 改为可选 | 不填则自动生成 UUID |
| 待办查询参数变更 | `owner_type` / `owner_id` 统一为 `scope` / `user_id`；列表接口新增 `contact_id`、`todo_type`、`is_archived` 过滤 |
| 历史搜索参数放宽 | `start_time` / `end_time` 改为可选；新增 `scope` / `user_id` 支持 |
| 所有 POST 端点 | 接受 JSON 格式请求体（文件上传使用 `application/octet-stream` 原始字节流） |
| `scope` 取值扩展 | v0.5.1 起新增 `Contact` 和 `Public` |
| 文件上传 | Content-Type 固定为 `application/octet-stream`；最大大小 20MB；支持 MD5 去重；异步文本提取 |

---

## 附录

### 时间格式说明

所有时间字段使用 RFC3339 格式：

```
2026-06-05T10:30:00+08:00
2026-06-05T10:30:00Z
```

### 错误处理

所有 API 在失败时返回统一格式：

```json
{
  "success": false,
  "data": null,
  "error": "错误描述信息"
}
```

常见 HTTP 状态码：

| 状态码 | 含义 |
|--------|------|
| 200 | 成功 |
| 400 | 请求参数错误 |
| 401 | 认证失败 |
| 404 | 资源不存在 |
| 409 | 冲突（如重复数据或操作冲突） |
| 413 | 请求体过大（如文件超过 20MB） |
| 500 | 服务器内部错误 |

### 超时说明

HTTP 请求超时时间为可配置项，默认 60 秒，最小 30 秒。长对话处理或文件上传等耗时操作建议适当增加客户端超时时间。

---

## 版本信息

| 版本 | 日期 | 变更内容 |
|------|------|----------|
| v0.5.3 | 2026-07-16 | 记忆生命周期管理（auto_invalid_at 自动失效、promote/demote 提升降级、待办 deadline 到期失效）；事件去重三重防护（analyzer 去重检查、对话状态过滤、LLM 重要性传递）；心跳维护阶段执行顺序优化 |
| v0.5.2 | 2026-07-15 | 心跳 trigger API 新增 `maintenance_only` 参数；联系人搜索 API 新增 `include_archived` 参数；事件搜索 API 新增 `include_archived` 和 `archived_month` 参数；待办列表 API 新增 `include_archived` 参数；心跳稳定性修复（RAII guard）；联系人匹配优化；upsert 数据保护 |
| v0.5.1 | 2026-06-05 | 新增虚拟实体管理 API（6 端点）；新增事件合并管理 API（2 端点）；心跳报告与状态字段扩展；Todo 归档/提醒机制与新字段；文件异步处理与 MD5 去重；Scope 扩展 Contact/Public |
| v0.5.0 | 2026-06-02 | 新增文件管理 API；Todo 去重优化；事件独立标题；联系人提取增强；长对话健壮性；正则回退提取；语义搜索增强 |
| v0.4.0 | 2026-05-29 | 新增多源 LLM 管理 API；标准化待办 API 路径；新增 `scope` 参数 |
| v0.2.0 | 2026-04-20 | 初始 API 参考文档 |