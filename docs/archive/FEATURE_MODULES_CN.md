# ShouXin 功能模块文档 (v0.5.3)

ShouXin（守心）是一个基于三库架构（字典库、关系库、向量库）的个人记忆与关系管理系统。本文档描述了项目的全部14个功能模块，涵盖其API端点、内部架构和关键数据结构。

---

## 模块 1：记忆管理

### 模块用途

核心模块，负责添加对话数据，以及跨联系人、事件、历史和语义内容的多维度搜索。通过 LLM 从自然语言对话中提取结构化记忆元素（人员、事件、待办、事实、学习内容），并分发到三库存储系统中。

### API 端点

#### POST /api/memory/add

添加对话，触发提取和存储。

**请求体 (AddConversationRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| conversation | string | 是 | 对话文本内容 |
| user_id | string | 否 | 用户ID，用于作用域隔离 |
| scope | string | 否 | 操作作用域：`User` 或 `Agent`（默认：`User`） |

**响应 (AddConversationResponse)：**

```json
{
  "success": true,
  "data": {
    "contacts": [],
    "events": [],
    "learnings": [],
    "conversation_id": "550e8400-e29b-41d4-a716-446655440000"
  }
}
```

v0.5.1 中，此端点使用 `add_async`：对话立即存储，LLM 提取通过 `BackgroundTaskQueue` 在后台异步执行。响应立即返回 `conversation_id`。

#### GET /api/memory/extract_status/:conversation_id

获取对话的提取处理状态。

**响应：**

```json
{
  "success": true,
  "data": {
    "conversation_id": "550e8400-...",
    "status": "completed",
    "extraction_error": null,
    "created_at": "2026-06-05T10:00:00Z",
    "content_preview": "对话内容的前200个字符..."
  }
}
```

状态值：`pending`（待处理）、`processing`（提取中）、`completed`（已提取）、`failed`（失败）。

#### POST /api/memory/search/contacts

按姓名或关键词搜索联系人。

**请求体 (SearchContactsRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| query | string | 是 | 搜索查询文本 |
| limit | number | 否 | 结果数量限制（默认：10） |
| scope | string | 否 | `user` 或 `agent`（默认：`user`） |
| user_id | string | 否 | 用户ID，用于作用域隔离 |
| include_archived | boolean | 否 | 是否包含已归档（冷热分离）的联系人 |

**响应：** `ContactDetail` 对象数组，包含 `contact_id`、`name`、`aliases`、`background`、`last_interaction_at`、`activity_score`、`related_contacts`、`relationship_tags`。

#### POST /api/memory/search/events

搜索事件，支持灵活过滤。

**请求体 (SearchEventsRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| user_id | string | 否 | 用户ID |
| event_type | string | 否 | 事件类型过滤 |
| start_time | string | 否 | RFC3339 格式的开始时间 |
| end_time | string | 否 | RFC3339 格式的结束时间 |
| importance | string | 否 | 重要程度：`high`、`medium`、`low` |
| status | string | 否 | 状态：`pending`、`completed` |
| limit | number | 否 | 结果数量限制（默认：10） |
| scope | string | 否 | 操作作用域 |
| include_archived | boolean | 否 | 是否包含已归档事件 |
| archived_month | string | 否 | 按归档月份过滤（如 "2026-06"） |

#### POST /api/memory/search/history

按时间范围搜索对话历史。

**请求体 (SearchHistoryRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| start_time | string | 否 | RFC3339 格式的开始时间（默认：epoch） |
| end_time | string | 否 | RFC3339 格式的结束时间（默认：当前时间） |
| scope | string | 否 | 操作作用域 |
| user_id | string | 否 | 用户ID |

#### POST /api/memory/search/semantic

跨学习资料的语义（向量）搜索。

**请求体 (SemanticSearchRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| query | string | 是 | 搜索查询文本 |
| limit | number | 否 | 结果数量限制（默认：10） |
| scope | string | 否 | 操作作用域 |
| user_id | string | 否 | 用户ID |
| source_types | string[] | 否 | 按来源类型过滤（如 `["event", "learning"]`） |
| conversation_id | string | 否 | 按特定对话过滤（v0.5.1 新增） |

**响应：** `VectorSearchResult` 数组，包含 `material`（LearningMaterial 对象）和 `score`（余弦相似度，0-1）。

### 内部架构

1. **提取流水线**：`LlmMemoryExtractor` 使用 LLM 处理对话文本，通过结构化 JSON 输出提示词引导。提取器使用详细的系统提示词，包含针对中文人名和事件提取的 few-shot 示例。

2. **LLM 输出后处理 (`LlmOutputSanitizer`)**：处理 GGUF 模型输出中的乱码问题：
   - 字符集检测：检测西里尔字符占比 > 5% 或中文字符占比 < 30% 时判定为乱码
   - JSON 片段提取：从乱码输出中提取第一个 `{...}` 块
   - 启发式意图提取：当 LLM 失败时，使用正则表达式从对话文本中提取待办更新意图

3. **数据分发器 (DataDispatcher)**：将提取的元素路由到三个库：
   - 字典库（Sled）：完整对话文本、联系人总表、联系人事件历史
   - 关系库（SQLite/WAL）：联系人索引、事件索引、待办事项
   - 向量库（HNSW/Embedded）：学习资料和事件嵌入向量

4. **异步队列（v0.5.1）**：`BackgroundTaskQueue` 实现非阻塞提取。对话立即持久化，提取任务排队后由工作线程处理。

5. **降级提取**：当 LLM 提取失败时，系统回退到基于正则表达式的提取，使用中文姓氏模式、称谓模式和事件关键词匹配。

6. **联系人提取与匹配（v0.5.2）**：
   - 联系人提取使用 `find_contact_by_any_name` 按 name OR aliases 匹配已有联系人，避免 LLM 用不同 name 引用同一人时创建重复联系人
   - upsert 时保留已有业务字段（background、notes、related_contact_ids、relationship_tags、relation_weight），不被空值覆盖
   - 别名同步写入 `contact_alias_index` 表

7. **联系人生命周期管理（v0.5.2）**：
   - 热表归档（MAX_ACTIVE_CONTACTS=50）：超限时将最久未交互的联系人标记为 `is_archived=1`（软删除），而非物理删除
   - 冷热表归档（`archive_stale_contacts`）：180天未交互的联系人自动归档
   - 搜索 API 支持 `include_archived` 参数，控制是否返回已归档联系人
   - 虚拟角色检查：提取联系人时先检查是否为虚拟角色（如 ChatGPT、GPT 等），避免误提取

---

## 模块 2：待办管理

### 模块用途

管理待办事项，支持 CRUD 操作、写入时触发归档、提醒生成和冷热分离。

### API 端点

#### POST /api/todos

创建新的待办事项。触发写入时归档和提醒生成。

**请求体 (AddTodoRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| todo_id | string | 否 | 客户端提供的ID（未提供时服务端生成UUID） |
| contact_ids | string[] | 否 | 关联的联系人ID列表 |
| title | string | 是 | 待办标题（最大255字符，自动trim） |
| description | string | 否 | 详细描述 |
| deadline | string | 是 | RFC3339 格式的截止时间 |
| importance | string | 否 | `high`、`medium`、`low`、`short_term`、`long_term`、`critical`（默认：`medium`） |
| source_event_id | string | 否 | 来源事件ID引用 |
| scope | string | 否 | 操作作用域 |
| user_id | string | 否 | 用户ID |

**校验规则：**
- 标题自动 trim，验证非空
- 标题截断到 255 字符（UTF-8 安全）
- 非法 importance 值降级为 `medium`
- 重复检查：相同标题 + 相同截止时间（1分钟精度）+ 相同所有者 = 409 冲突

**响应：** 创建的 `TodoItem` 对象。

#### GET /api/todos

带过滤条件的待办列表。

**查询参数 (ListTodosRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| status | string | 否 | `pending`、`completed`、`auto_invalid` |
| contact_id | string | 否 | 按联系人ID过滤 |
| limit | number | 否 | 结果数量限制（默认：100） |
| scope | string | 否 | 操作作用域 |
| user_id | string | 否 | 用户ID |
| include_archived | boolean | 否 | 是否包含已归档待办 |

**响应：** `TodoItem` 对象数组，按截止时间升序排列。

#### GET /api/todos/:id

获取单个待办详情。

**查询参数 (ScopeQueryParams)：** `scope`、`user_id`（可选，未提供时从待办所有者自动推断）。

#### PUT /api/todos/:id/complete

将待办标记为已完成。设置 `completed_at` 和 `last_updated_at` 为当前时间。

#### DELETE /api/todos/:id

删除待办。需要所有者作用域匹配。

### 内部架构

**写入时归档触发**：调用 `add_todo` 时，系统检查待办数量：
- 若 `pending_count > 50`：归档陈旧待办（30天以上未更新），仅保留最近 50 个待处理
- 若 `pending_count > 30`：生成提醒待办（type=`reminder`），标题为 `"⚠️ 你有 N 个待办未处理，请及时处理"`，仅当日无提醒时生成

**提醒去重**：通过 `has_recent_reminder()` 检查今日是否已有提醒。

**设计原则**：仅归档 30 天以上的陈旧待办，不会归档新创建的待办，确保不丢失关键数据。

### 关键数据结构：TodoItem

```rust
pub struct TodoItem {
    pub todo_id: String,              // 唯一ID
    pub contact_ids: Vec<String>,     // 关联联系人ID（JSON数组）
    pub owner_id: String,             // 所有者ID
    pub owner_type: OwnerType,        // User、Agent、Contact、Public
    pub title: String,                // 标题
    pub description: Option<String>,  // 描述
    pub deadline: DateTime<Utc>,      // 截止时间
    pub status: String,               // pending、completed、auto_invalid
    pub created_at: DateTime<Utc>,    // 创建时间
    pub completed_at: Option<DateTime<Utc>>, // 完成时间
    pub importance: String,           // high、medium、low、short_term、long_term、critical
    pub source_event_id: Option<String>, // 来源事件ID
    pub todo_type: String,            // normal、ambiguity、confirmation、reminder
    pub entity_type: Option<String>,  // 关联实体类型
    pub entity_id: Option<String>,    // 关联实体ID
    pub is_archived: bool,            // 冷热归档标记
    pub archived_at: Option<DateTime<Utc>>, // 归档时间
    pub last_updated_at: DateTime<Utc>, // 最后更新时间
}
```

**唯一约束**：`(title, owner_id, owner_type, deadline)` WHERE `status='pending'`（通过 `INSERT OR IGNORE` 实现）。

---

## 模块 3：心跳系统

### 模块用途

周期性后台任务系统，执行维护操作（数据清理、去重、合并）和基于 LLM 的分析（画像更新、问题生成）。采用拆分架构，将维护任务与 LLM 任务解耦。

### API 端点

#### POST /api/heartbeat/trigger

手动触发一次心跳执行。支持维护和 LLM 两种任务。

**请求体：** 可选的 JSON 体（非必填）。

**响应：** `HeartbeatReport` 对象。若心跳正在执行中，返回 409 冲突。

#### GET /api/heartbeat/pending_questions

获取心跳生成的所有待处理问题。

**响应：** `Question` 对象数组，包含 `id`、`priority`、`category`、`title`、`detail`、`context`、`related_contact_ids`、`related_event_ids`。

#### GET /api/heartbeat/status

获取当前心跳系统状态。

**响应：**

```json
{
  "success": true,
  "data": {
    "is_running": true,
    "last_heartbeat": "2026-06-05T09:00:00Z",
    "last_memory_write": "2026-06-05T10:00:00Z",
    "pending_questions_count": 3,
    "next_heartbeat_in_secs": 360,
    "consecutive_skips": 0,
    "last_skipped": false,
    "available_memory_mb": 8192
  }
}
```

#### POST /api/heartbeat/answer

回答一个待处理问题。

**请求体 (AnswerRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| question_id | string | 是 | 要回答的问题ID |
| answer | string | 是 | 回答文本 |

### 内部架构

**拆分架构**：
- **维护心跳**（30秒超时）：纯 SQL 任务，从不调用 LLM。包括陈旧联系人清理、事件去重/清理、联系人合并、歧义检测、待办冷热管理。
- **LLM 心跳**（120秒超时）：基于 LLM 的任务，包括画像分析和问题生成。

**调度器**：`HeartbeatScheduler` 按可配置间隔运行（默认：360秒）。特性：
- 自适应间隔，带内存阈值检查（最低 4GB 可用内存）
- 最大连续跳过次数限制（3次），超过后强制执行
- 并发控制：`AtomicBool` 防止重复执行

**v0.5.2 增强：**
- **ExecutingGuard RAII 结构**：确保心跳 executing 标志在任何路径（包括 panic/超时）下都被清除，避免标志卡死导致后续心跳永久返回 409
- **孤立向量记忆清理**：维护阶段检查 SQLite event_id 存在性，删除向量库中对应的孤立记忆（event 已被清理但向量未删除的情况）
- **LLM 画像 JSON 兼容**：画像生成兼容 markdown 代码块包裹的 JSON 输出（`extract_json_from_response`）
- **关系字段兼容**：`social_relations`/`potential_relations` 字段兼容字符串数组和对象数组两种 LLM 输出格式

**v0.5.3 增强：**
- **记忆生命周期管理**：事件创建时根据 importance_score 自动计算 `auto_invalid_at` 失效时间（critical 永不失效/长期30天/短期7天/低价值3天）
- **记忆提升/降级**：基于事件被引用次数自动调整 importance_score（短期被3+事件引用→提升为长期；长期60天无引用→降级为短期）
- **待办 deadline 到期失效**：将 deadline 已过期的 pending 待办标记为 `auto_invalid`
- **维护阶段执行顺序优化**：promote/demote 移到 cleanup 之前执行，避免被频繁引用的短期记忆被先归档
- **事件去重三重防护**：background_queue 查重 + analyzer 查重 + 对话状态过滤（跳过已处理对话）
- **LLM 重要性传递**：心跳路径从 LLM 提取结果获取 importance，而非硬编码 0.5

### 关键数据结构：HeartbeatReport

```rust
pub struct HeartbeatReport {
    pub started_at: DateTime<Utc>,      // 开始时间
    pub completed_at: DateTime<Utc>,    // 完成时间
    pub actions_generated: usize,       // 生成的操作数
    pub questions_created: usize,       // 创建的问题数
    pub contacts_updated: usize,        // 更新的联系人数
    pub events_cleaned: usize,          // 清理的事件数
    pub events_merged: usize,           // 合并的事件数
    pub contacts_merged: usize,         // 合并的联系人数
    pub ambiguities_detected: usize,    // 检测到的歧义数
    pub profiles_updated: usize,        // 更新的画像数
    pub skipped: bool,                  // 是否被跳过
    pub skip_reason: Option<String>,    // 跳过原因
    pub duration_ms: u64,               // 执行耗时（毫秒）
    pub maintenance_only: bool,         // 是否仅执行维护阶段
}
```

### 配置

```toml
[heartbeat]
enabled = true
base_interval_secs = 360
min_interval_secs = 300
max_interval_secs = 600
adaptive = false
idle_threshold_secs = 300
```

---

## 模块 4：文件管理

### 模块用途

处理文件上传、元数据检索、文本提取和删除。使用基于 MD5 的去重机制和异步后台文本提取处理。

### API 端点

#### POST /api/files/upload

上传文件进行处理。原始字节在请求体中，元数据在查询参数中。

**查询参数：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| filename | string | 否 | 文件名（默认：`upload.txt`） |
| scope | string | 否 | 所有者作用域（默认：`agent`） |
| user_id | string | 否 | 用户ID（默认：`primary_agent`） |
| target_type | string | 否 | 文件关联的目标实体类型 |
| target_id | string | 否 | 文件关联的目标实体ID |

**约束：**
- 最大文件大小：20MB
- 支持的文件类型：`txt`、`md`、`code`、`pdf`、`doc`、`xls`、`csv`

**MD5 去重：** 相同文件内容 + 相同所有者返回已有的 `file_id`：

```json
{
  "success": true,
  "data": {
    "file_id": "existing-uuid",
    "status": "completed",
    "dedup": true
  }
}
```

**新上传：**

```json
{
  "success": true,
  "data": {
    "file_id": "new-uuid",
    "status": "pending"
  }
}
```

#### GET /api/files/:file_id

获取文件元数据。返回 `FileIndex` 对象。

#### GET /api/files/:file_id/text

获取提取的文本内容。返回 `{"file_id": "...", "full_text": "..."}`。

#### DELETE /api/files/:file_id

删除文件及其文本。同时删除文件关联关系。

### 内部架构

**异步处理**：数据库插入是同步的（元数据立即可用），文件文本提取通过 `tokio::spawn` 异步执行：
1. 文件内容保存到 `./temp_uploads/<file_id>`
2. 记录插入 `file_index` 表，状态为 `pending`
3. 后台任务启动 `FileProcessor::process_file()` 提取文本，将状态更新为 `completed` 或 `failed`

**MD5 去重**：`(md5_hash, owner_type, owner_id)` 上的唯一索引防止重复上传。

### 关键数据结构：FileIndex

```rust
pub struct FileIndex {
    pub file_id: String,                // 文件唯一ID
    pub file_name: String,              // 文件名
    pub file_type: String,              // txt、md、code、pdf、doc、xls、csv
    pub mime_type: Option<String>,      // MIME类型
    pub file_size: Option<i64>,         // 文件大小
    pub md5_hash: Option<String>,       // MD5哈希
    pub owner_type: String,             // 所有者类型
    pub owner_id: String,               // 所有者ID
    pub uploaded_at: Option<String>,    // 上传时间
    pub processed_at: Option<String>,   // 处理完成时间
    pub status: String,                 // pending、completed、failed
    pub error_message: Option<String>,  // 错误信息
    pub original_path: Option<String>,  // 原始文件路径
    pub extracted_text_summary: Option<String>, // 提取文本摘要
}
```

---

## 模块 5：虚拟实体管理

### 模块用途

管理虚拟/虚构角色作为联系人，支持别名机制。角色以 `ContactIndex` 记录存储，标记 `is_virtual=true`，并关联别名用于名称匹配。

### API 端点

#### GET /api/virtual_entities

列出所有虚拟实体。

**响应：** `VirtualCharacterInfo` 数组，包含 `contact`（ContactIndex）和 `aliases`（VirtualCharacterAlias 数组）。

#### GET /api/virtual_entities/:id

按联系人ID获取单个虚拟实体。

#### POST /api/virtual_entities

添加新的虚拟角色。

**请求体 (AddVirtualCharacterRequest)：**

```json
{
  "name": "林克",
  "aliases": [
    {"name": "Link", "source_work": "塞尔达传说", "alias_type": "official"},
    {"name": "时之勇者", "source_work": "塞尔达传说", "alias_type": "nickname"}
  ]
}
```

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| name | string | 是 | 角色名称 |
| aliases | AliasInput[] | 是 | 别名列表 |

**AliasInput：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| name | string | 是 | 别名名称 |
| source_work | string | 否 | 来源作品（如游戏、书籍） |
| alias_type | string | 否 | 别名类型（如 `official`、`nickname`、`localized`、`title`） |

#### PUT /api/virtual_entities/:id

更新虚拟实体。支持部分更新：`name` 和 `aliases` 均为可选。若提供 `aliases`，将替换所有现有别名。

**请求体 (UpdateVirtualCharacterRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| name | string | 否 | 新角色名称 |
| aliases | AliasInput[] | 否 | 新别名列表（替换全部） |

#### DELETE /api/virtual_entities/:id

删除虚拟实体及其所有别名。

#### GET /api/virtual_entities/check/:name

检查名称是否匹配任何虚拟实体。同时搜索别名名称和联系人名称（不区分大小写）。

**响应：** 若找到，返回 `VirtualCharacterInfo`；否则返回错误。

### 内部架构

- 虚拟角色以 `ContactIndex` 记录存储，标记 `is_virtual=true`，`owner_id="system"`（内置）或 `owner_id="default_user"`（用户创建）。
- 别名存储在 `virtual_character_aliases` 表中。
- 内置虚拟角色包括：林克、ChatGPT、马里奥、皮卡丘、孙悟空、曹操、蝙蝠侠、钢铁侠、克劳德、哆啦A梦、柯南。
- 内置角色在服务启动时初始化（若尚未存在）。

### 关键数据结构

```rust
pub struct VirtualCharacterAlias {
    pub id: i64,                       // 别名ID
    pub contact_id: String,            // 关联联系人ID
    pub alias_name: String,            // 别名名称
    pub source_work: Option<String>,   // 来源作品
    pub alias_type: String,            // 别名类型
    pub created_at: DateTime<Utc>,     // 创建时间
}

pub struct VirtualCharacterInfo {
    pub contact: ContactIndex,         // 联系人索引
    pub aliases: Vec<VirtualCharacterAlias>, // 别名列表
}
```

---

## 模块 6：事件合并管理

### 模块用途

管理事件语义去重的合并工作流。当新事件添加时，系统检查与已有事件的语义相似度，根据相似度自动合并（相似度 > 0.90）或生成待确认合并记录（相似度 0.75-0.90）。

### API 端点

#### POST /api/events/pending_merges

获取需要人工确认的待处理事件合并项。

**请求体 (GetPendingMergesRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| owner_type | string | 否 | `User`、`Agent`、`Contact`、`Public` |
| owner_id | string | 否 | 所有者ID |
| limit | number | 否 | 结果数量限制（默认：10） |

**响应：** `PendingMergeItem` 数组：

```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "source_event_id": "pending_uuid",
      "target_event_id": "existing_event_uuid",
      "source_event_summary": "新事件摘要",
      "target_event_summary": "已有事件摘要",
      "similarity_score": 0.82,
      "status": "pending",
      "created_at": "2026-06-05T10:00:00Z"
    }
  ]
}
```

#### POST /api/events/confirm_merge

确认或拒绝待处理的事件合并。

**请求体 (ConfirmMergeRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| merge_id | number | 是 | 待处理合并记录ID |
| action | string | 是 | `merge`（合并到目标事件）或 `create`（创建为新事件） |
| source_event_summary | string | 否 | 来源事件摘要（action=`create` 时必填） |

**响应：**

```json
{
  "success": true,
  "data": {
    "success": true,
    "message": "事件已合并",
    "event_id": "merged_event_uuid"
  }
}
```

### 内部架构

**语义合并流水线（v0.5.1）**：
1. 新事件添加时，系统通过 `FastEmbedder` 生成嵌入向量
2. 在向量库中搜索相似事件（top 3）
3. 若相似度 > 0.90：自动合并到已有事件
4. 若相似度 0.75-0.90：生成待确认合并记录
5. 若相似度 < 0.75：作为新事件处理

### 关键数据结构：PendingMergeItem

```rust
pub struct PendingMergeItem {
    pub id: i64,                          // 记录ID
    pub source_event_id: String,          // 来源事件ID
    pub target_event_id: Option<String>,  // 目标事件ID
    pub source_event_summary: String,     // 来源事件摘要
    pub target_event_summary: Option<String>, // 目标事件摘要
    pub similarity_score: f32,            // 相似度分数
    pub status: String,                   // pending、merged、created、dismissed
    pub created_at: String,               // 创建时间
}
```

---

## 模块 7：向量搜索

### 模块用途

通过向量嵌入提供语义搜索。使用与记忆语义搜索相同的底层端点。

### API 端点

#### POST /api/vector/search

向量语义搜索。与 `/api/memory/search/semantic` 为同一端点。

**请求体 (SemanticSearchRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| query | string | 是 | 搜索查询文本 |
| limit | number | 否 | 结果数量限制（默认：10） |
| scope | string | 否 | 操作作用域 |
| user_id | string | 否 | 用户ID |
| source_types | string[] | 否 | 按来源类型过滤 |
| conversation_id | string | 否 | 按对话ID过滤 |

#### GET /api/vector/:id

按ID获取学习资料详情。

**响应：** `LearningMaterial` 对象。

### 内部架构

- **嵌入向量**：768 维向量，由 `intfloat/multilingual-e5-base` 通过 FastEmbed 生成
- **向量库**：可在 embedded（内存 usearch）和 HNSW（磁盘持久化）后端之间配置
- **搜索**：使用余弦相似度评分。结果按所有者作用域过滤，可额外按来源类型和对话ID过滤
- **回退机制**：若按所有者过滤的搜索结果为空，重试时不加所有者过滤

### 关键数据结构

```rust
pub struct LearningMaterial {
    pub id: u64,                             // 唯一ID
    pub content: String,                     // 内容
    pub summary: Option<String>,             // 摘要
    pub source_conversation_id: Option<String>, // 来源对话ID
    pub created_at: DateTime<Utc>,           // 创建时间
    pub material_type: String,               // knowledge、fact、preference、event、learning
    pub canonical_entity_id: Option<String>, // 规范实体ID
    pub keywords: Vec<String>,               // 关键词
    pub context_before_id: Option<String>,   // 上文ID
    pub context_after_id: Option<String>,    // 下文ID
    pub triples: Vec<KnowledgeTriple>,       // 知识三元组
    pub owner_type: OwnerType,               // 所有者类型
    pub owner_id: String,                    // 所有者ID
}

pub struct VectorSearchResult {
    pub material: LearningMaterial,  // 学习资料
    pub score: f32,                  // 余弦相似度，0-1
}
```

---

## 模块 8：智能搜索

### 模块用途

跨多个数据源（联系人、事件、历史、待办）的统一搜索。在单个响应中返回所有来源的聚合结果。

### API 端点

#### POST /api/search/smart

跨多种数据类型的统一搜索。

**请求体 (SmartSearchRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| query | string | 是 | 搜索查询文本 |
| limit | number | 否 | 每种类型的数量限制（默认：10，最大：20） |
| types | string[] | 否 | 搜索类型（默认：`["contacts", "events", "history", "todos"]`） |
| scope | string | 否 | 操作作用域 |
| user_id | string | 否 | 用户ID |

**响应 (SmartSearchResponse)：**

```json
{
  "success": true,
  "data": {
    "contacts": [...],
    "events": [...],
    "history": [...],
    "todos": [...]
  }
}
```

### 内部架构

智能搜索遍历请求的类型，分别执行搜索：
- **contacts**：使用 `search_contacts()` 查询字符串
- **events**：使用 `search_events()` 带搜索查询过滤
- **todos**：获取所有待办，在内存中按标题/描述子串匹配过滤
- **history**：获取最近 30 天的历史记录，在内存中过滤

每种类型限制为 `min(limit, 20)` 条结果，防止超时。

---

## 模块 9：内部 API

### 模块用途

供子代理注册和事件创建的内部 API。需要 Bearer Token 认证。

### API 端点

#### POST /internal/agent/register

注册子代理。

**认证：** `Authorization: Bearer {token}`（token 来自 `INTERNAL_API_TOKEN` 环境变量，无默认值，未配置 INTERNAL_API_TOKEN 环境变量时返回 500 错误）

**请求体 (AgentRegistrationRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| agent_id | string | 是 | 代理唯一ID |
| name | string | 是 | 代理显示名称 |
| description | string | 否 | 代理描述 |

**响应：** 成功返回 201 Created，代理已存在返回 409 Conflict。

#### POST /internal/event/add

添加内部事件。

**认证：** `Authorization: Bearer {token}`

**请求体 (InternalEventAddRequest)：**

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| description | string | 是 | 事件描述 |
| related_contacts | string[] | 是 | 关联联系人ID列表 |
| source_agent_id | string | 是 | 来源代理ID |
| owner_type | string | 否 | `Agent` 或 `User`（默认：`Agent`） |
| owner_id | string | 否 | 所有者ID（默认：`primary_agent`） |

**响应：** 201 Created，返回 `event_id`。

### 内部架构

- `InternalTokenAuth` 中间件提取 `Authorization: Bearer` 头部
- Token 与 `INTERNAL_API_TOKEN` 环境变量比较（无默认值，未配置 INTERNAL_API_TOKEN 环境变量时返回 500 错误）
- 代理注册同时创建 `ContactIndex` 和 `ContactTotal` 记录，标记 `is_agent=true`
- 内部事件使用 UUID 创建，解析时间，中等重要程度

---

## 模块 10：管理 API

### 模块用途

LLM 后端管理和状态监控的管理端点。

### API 端点

#### GET /admin/llm/status

获取 LLM 管理器状态。

**认证：** `X-Internal-Token` 头部（token 来自 `config.toml` 中 `[agent] internal_api_token`）

**响应：**

```json
{
  "success": true,
  "data": {
    "active_source": "ollama",
    "is_primary": true,
    "total_requests": 150,
    "total_failures": 3,
    "primary_healthy": true,
    "secondary_healthy": true,
    "auto_switch_enabled": true
  }
}
```

#### POST /admin/llm/switch

切换 LLM 后端。

**认证：** `X-Internal-Token` 头部

**请求体：**

```json
{
  "target": "primary"
}
```

| 字段 | 类型 | 必填 | 说明 |
|-------|------|------|------|
| target | string | 是 | `primary` 或 `secondary` |

**响应：**

```json
{
  "success": true,
  "data": {
    "switched_to": "primary"
  }
}
```

### 配置

管理 token 在 `config.toml` 中配置：

```toml
[agent]
primary_agent_id = "primary_agent_001"
internal_api_token = "your-secure-admin-token"
```

---

## 模块 11：健康检查

### 模块用途

简单的健康检查端点，用于监控和负载均衡器集成。

### API 端点

#### GET /health

**响应：**

```json
{
  "status": "ok",
  "time": "2026-06-05T10:00:00.123456789Z"
}
```

无需认证。

---

## 模块 12：LLM 系统

### 模块用途

多后端 LLM 客户端系统，支持主/备故障切换、失败自动切换和健康检查监控。

### 内部架构

**后端类型：**
- **GGUF**（`llama-cpp-2`）：本地模型推理，可配置上下文大小、线程数、批处理大小、GPU 层数
- **Ollama**：通过 HTTP API 连接 Ollama 服务，支持配置模型名称和端点
- **API**（OpenAI 兼容）：通过 HTTP API 连接任何 OpenAI 兼容端点，支持自定义头部和 API 密钥

**LlmClientManager：**
- 封装一个主客户端和一个可选备用客户端
- 使用 `AtomicUsize` 实现无锁的主/备切换
- 通过代理模式实现 `LlmClient` trait，所有请求通过活跃客户端
- 指数退避重试：最多 2 次尝试，延迟为 2^n * 1000ms
- 自动切换：主端连续失败达到 `max_consecutive_failures` 次后切换到备用端
- 后台健康检查：每 `health_interval_secs` 秒 ping 主端；主端恢复后自动切换回来
- 支持通过管理 API 手动切换

**ManagerStatus：**

```rust
pub struct ManagerStatus {
    pub active_source: String,              // "ollama"、"gguf"、"api"
    pub is_primary: bool,                   // 是否使用主端
    pub total_requests: u64,                // 总请求数
    pub total_failures: u64,                // 总失败数
    pub health_check_interval_secs: u64,    // 健康检查间隔
    pub primary_healthy: bool,              // 主端是否健康
    pub secondary_healthy: Option<bool>,    // 备用端是否健康
    pub auto_switch_enabled: bool,          // 是否启用自动切换
}
```

### 配置

```toml
[llm]
primary = "ollama"           # ollama、gguf、api
secondary = "api"            # 可选备用端
temperature = 0.7
max_tokens = 2000
timeout_secs = 60

[llm.ollama]
model = "llama3.2:3b"
endpoint = "http://localhost:11434"

[llm.gguf]
model_path = "models/Llama-3.2-3B-Instruct-Q5_K_M.gguf"
context_size = 2048
threads = 4

[llm.api]
endpoint = "https://api.openai.com/v1"
api_key = "sk-..."
model = "gpt-4o-mini"
```

---

## 模块 13：三库存储

### 模块用途

基础存储架构，将数据分离到三个专用库中，分别满足不同的访问模式和持久性需求。

### 架构

**1. 字典库（Sled）**
- 存储引擎：Sled（嵌入式键值存储，LSM 树）
- 用途：所有原始数据的永久存档
- 存储内容：`ContactTotal`（完整联系人记录）、`ConversationRecord`（原始对话文本）、`ContactEventRecord`（每个联系人的事件历史）、文件提取文本
- 数据永久保存，永不删除
- 优雅关闭：flush + close，防止锁残留

**2. 关系库（SQLite/WAL）**
- 存储引擎：SQLite，WAL journal 模式，64 最大连接数，5秒 busy timeout
- 用途：结构化查询的快速索引
- 存储内容：`ContactIndex`（轻量联系人索引）、`EventIndex`（事件元数据）、`TodoItem`、`FileIndex`、`Conversation` 元数据、虚拟实体别名、待处理事件合并
- 支持冷热分离：活跃记录留在热表，陈旧记录归档
- 通过 `owner_type` 和 `owner_id` 列实现按所有者隔离

**3. 向量库（HNSW/Embedded）**
- 存储引擎：可在 embedded（内存 usearch）和 HNSW（磁盘持久化）之间配置
- 用途：高维语义搜索
- 嵌入维度：768（intfloat/multilingual-e5-base）
- 距离度量：余弦相似度
- 存储内容：`LearningMaterial` 嵌入向量及元数据
- 自动去重：与已有资料的相似度 > 0.98 时跳过插入
- 通过 `SHOUXIN_STORE_TYPE` 环境变量配置（`embedded` 或 `hnsw`）

### 关键数据流

```
对话输入
    |
    v
LLM 记忆提取器 (LlmMemoryExtractor)
    |
    v
数据分发器 (DataDispatcher)
    |
    +---> 字典库 (Sled)：全文、联系人总表、事件历史
    |
    +---> 关系库 (SQLite)：联系人索引、事件索引、待办
    |
    +---> 向量库 (HNSW/Embedded)：学习资料嵌入向量
```

### 配置

```toml
[storage]
data_dir = "./data"
dictionary_dir = "dictionary"
relational_dir = "relational"
vector_dir = "vector"
relational_db_file = "relational.db"
vector_index_file = "vectors.hnsw"
```

---

## 模块 14：记忆提取器

### 模块用途

通过 LLM 结合提示工程、降级正则提取和输出后处理，从自然语言对话文本中提取结构化记忆元素（人员、事件、待办、事实、学习内容）。

### 内部架构

**LlmMemoryExtractor：**
- 使用详细的系统提示词，包含中文提取的 few-shot 示例
- 提示词引导 LLM 输出结构化 JSON，包含 `people`、`events` 和 `todo_update_intent` 字段
- 对话文本在发送到 LLM 前截断到 1024 字符
- 提取超时：600 秒（同步模式）

**提取流水线：**
1. LLM 提取，使用结构化 JSON 输出
2. 响应解析：剥离 markdown 代码围栏，提取 JSON 块
3. **LlmOutputSanitizer** 检查乱码输出：
   - 字符集检测：输出中西里尔字符 > 5% 或中文字符 < 30% 时标记为乱码
   - JSON 片段提取：在乱码输出中查找 `{...}` 块，将西里尔字符替换为空格
   - 启发式待办意图提取：使用正则匹配截止时间变更模式
4. LLM 失败时的降级提取：
   - 中文姓氏 + 称谓模式匹配（如 "王经理"、"赵总"、"小丽"）
   - 昵称模式："老X"、"小X"
   - 英文名模式：`[A-Z][a-z]+ [A-Z][a-z]+`
   - 事件关键词匹配（如 "会议"、"讨论"、"评审"、"开发"）
   - 复合关系提取："X的女儿"、"X的秘书"

**联系人验证：**
- 两阶段验证：规则过滤 + LLM 验证
- 规则过滤检查：空名称、单字名称、垃圾词、动作后缀、非人名词汇
- LLM 验证：将提取的名称发送回 LLM 验证（"这是人名吗？"）
- 名称截断修复：`fix_name_truncation()` 移除错误附加到名称的常见词后缀

**多段提取：**
- 对于长对话（> 80 字符），分段文本并从每段中提取人员
- 合并结果并去重

**数据分发器：**
- 将提取的元素路由到三个库：
  - **字典库**：完整对话文本、联系人总表、带情感分析的联系人事件历史
  - **关系库**：联系人索引（按名称+所有者去重）、事件索引（按摘要+所有者去重）、活跃联系人管理（最多 50 个）
  - **向量库**：学习资料（质量阈值 >= 0.8）、嵌入向量生成、基于相似度的去重、实体规范化和摘要

**提取元素类型：**

```rust
pub struct ExtractedElements {
    pub conversation_text: String,           // 对话文本
    pub people: Vec<ExtractedPerson>,        // 提取的人员
    pub events: Vec<ExtractedEvent>,         // 提取的事件
    pub todos: Vec<ExtractedTodo>,           // 提取的待办
    pub facts: Vec<ExtractedFact>,           // 提取的事实
    pub learnings: Vec<ExtractedLearning>,   // 提取的学习内容
    pub todo_update_intent: Option<TodoUpdateIntent>, // 待办更新意图
}

pub struct ExtractedPerson {
    pub contact_id: String,         // 联系人ID
    pub name: String,               // 姓名
    pub aliases: Vec<String>,       // 别名
    pub role: Option<String>,       // 角色
    pub description: Option<String>, // 描述
    pub contact_type: String,       // 联系人类型
}

pub struct ExtractedEvent {
    pub event_id: String,           // 事件ID
    pub title: Option<String>,      // 标题
    pub description: String,        // 描述
    pub time: Option<String>,       // 时间
    pub time_info: Option<String>,  // 时间信息
    pub location: Option<String>,   // 地点
    pub importance: f32,            // 重要程度
    pub importance_label: String,   // 重要程度标签
    pub start_time: Option<String>, // 开始时间
    pub end_time: Option<String>,   // 结束时间
    pub related_person_ids: Vec<String>, // 关联人员ID
}

pub struct TodoUpdateIntent {
    pub title_hint: Option<String>,    // 标题提示
    pub new_deadline: Option<String>,  // 新截止时间
}
```

**LlmOutputSanitizer 方法：**

| 方法 | 说明 |
|--------|-------------|
| `has_garbage_chars(text)` | 检测输出中西里尔字符 > 5% 或中文 < 30% |
| `extract_json_fragment(text)` | 提取 `{...}` 块，将西里尔字符替换为空格 |
| `extract_todo_intent_heuristic(text)` | 基于正则的截止时间变更意图提取 |

---

## 附录：服务配置

### config.toml 示例

```toml
[llm]
primary = "ollama"
secondary = "api"
temperature = 0.7
max_tokens = 2000
timeout_secs = 60

[llm.ollama]
model = "llama3.2:3b"
endpoint = "http://localhost:11434"

[llm.gguf]
model_path = "models/Llama-3.2-3B-Instruct-Q5_K_M.gguf"
context_size = 2048
threads = 4

[llm.api]
endpoint = "https://api.openai.com/v1"
api_key = "sk-..."
model = "gpt-4o-mini"

[agent]
primary_agent_id = "primary_agent_001"
internal_api_token = "your-admin-token"

[heartbeat]
enabled = true
base_interval_secs = 1800
adaptive = true

[storage]
data_dir = "./data"

[logging]
level = "info"
dir = "./logs"
```

### 环境变量

| 变量 | 说明 |
|----------|-------------|
| `SHOUXIN_STORE_TYPE` | 向量库类型：`embedded` 或 `hnsw` |
| `SHOUXIN_CONFIG` | config.toml 文件路径 |
| `FASTEMBED_CACHE_DIR` | FastEmbed 模型缓存目录 |
| `INTERNAL_API_TOKEN` | 内部 API 认证 Token |
| `SHOUXIN_<SECTION>__<KEY>` | 覆盖任意配置值（双下划线分隔） |

### 服务启动

```
cargo run --release -- --host 127.0.0.1:3000
```

服务启动三个主要组件：
- **API 线程**：HTTP 服务，监听指定地址，带可配置超时中间件
- **后台队列工作线程**：通过 `BackgroundTaskQueue` 处理异步提取任务
- **心跳调度器**：周期性维护和 LLM 任务

Ctrl+C 优雅关闭顺序：向量库持久化、SQLite WAL checkpoint、sled 字典库 flush 和 close。

---

## v0.5.2 变更日志 (2026-07-15)

### 严重修复
- **UTF-8 字符串切片 panic**：analyzer.rs 和 three_library_manager.rs 中 `&str[..200]` 在中文字符边界处 panic，改用 `chars().take(200).collect()`
- **心跳 executing 标志卡死**：panic 后标志未清除导致心跳永久返回 409，引入 `ExecutingGuard` RAII 结构确保自动清除
- **联系人匹配逻辑缺陷**：`search_contacts_by_name` 仅按 name 精确匹配，改用 `find_contact_by_any_name` 按 name OR aliases 匹配
- **upsert 覆盖业务字段**：`upsert_contact_index` 的 ON CONFLICT 用空值覆盖已有 background/notes 等字段，修复为保留已有值

### 中等修复
- 6 处 SELECT 查询补全 is_archived/archived_at/activity_score 字段
- 画像 JSON 解析兼容 markdown 代码块
- 关系字段兼容字符串数组和对象数组
- 别名索引表 contact_alias_index 写入逻辑
- 热表归档从物理 DELETE 改为 UPDATE is_archived=1
- 孤立向量记忆清理（心跳维护阶段）
- 虚拟角色误提取防护
- confirm_merge 状态更新修复
- MAX_ACTIVE_CONTACTS 限制生效（从死代码移至 write_extraction_results）

### 配置变更
- batch_size 从 2048 调整为 4096（仅影响 GGUF LLM 推理）
- vector_dir 配置生效（原使用硬编码默认值）

---

## v0.5.3 变更日志 (2026-07-16)

### 严重修复：记忆生命周期

- **待办 deadline 到期失效**：原 `invalidate_expired_todos()` 未实现，deadline 到期的 pending 待办永不失效。新增方法并在心跳维护中调用，将过期 pending 标记为 `auto_invalid`
- **`auto_invalid_at` 死字段激活**：原字段全为 NULL，事件永不自动失效。新增 `compute_auto_invalid_at()` 方法，在所有7个事件创建点调用，根据 importance_score 自动计算失效时间（critical 永不失效/长期30天/短期7天/低价值3天）
- **`cleanup_expired_events` 双路径归档**：fallback 归档路径未设置 `is_auto_invalid=1`，修复为双路径均标记
- **记忆 promote/demote 实现**：原 importance_score 创建后不可变。新增 `promote_demote_events()` 方法，基于事件被引用次数自动提升/降级
- **stale 待办归档计数偏差**：原代码先插入 reminder 再归档，导致 pending 总数超阈值。反转为先归档后提醒

### 严重修复：事件去重

- **analyzer 硬编码 importance**：心跳路径硬编码 `importance_score=0.5`，即使 LLM 判定为重要(0.9)也会创建0.5分版本。修复为从 LLM 提取结果获取 importance
- **analyzer 无去重检查**：心跳路径直接 insert，不检查是否已创建相同事件。新增 `search_events_by_summary` 去重检查
- **`get_conversations_since` 不过滤状态**：心跳会重新处理 background_queue 已处理的对话。修复为只返回 status=pending 或 failed 的对话

### 中等修复

- **critical 阈值边界 bug**：`>= 0.95` 在 f32 精度下将 0.95 误判为 critical，改为 `> 0.95`
- **promote 执行顺序冲突**：promote 在 cleanup 之后执行导致短期记忆被先归档，移到 cleanup 之前
- **promote 的 created_at 限制**：去掉 `created_at < 7天前` 限制，只看是否被近期3+事件引用

### 验证结果

- 生命周期测试：11/11 PASS（短期7天归档、长期30天归档、低重要性删除、待办deadline失效、auto_invalid_at设置、is_auto_invalid标记、promote/demote、stale待办归档）
- 去重测试：3轮全部 PASS（3次提交相同对话只创建1个事件、心跳未创建重复事件、全库无重复summary）