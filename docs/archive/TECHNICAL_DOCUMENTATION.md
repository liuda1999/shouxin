# ShouXin 项目技术文档

> **版本**: v0.5.3
> **更新日期**: 2026-07-16

---

## 一、项目概述

ShouXin（守心）v0.5.3 是一个以时间为轴、三库分离、纯本地运行的认知记忆系统。基于 Rust 构建，提供 LLM 驱动的记忆提取、三库存储架构和 REST API 接口。

### 核心特性

| 特性 | 说明 |
|------|------|
| **纯本地运行** | 无需联网，无 Docker，无云服务 |
| **LLM 驱动提取** | 多后端 LLM 自动提取联系人、事件、待办等结构化信息 |
| **三库存储架构** | 字典库（永久全量）、关系库（快速索引）、向量库（语义搜索） |
| **REST API** | Axum 0.7 构建的 HTTP API 服务 |
| **多源 LLM** | 支持 GGUF、Ollama、API 三种后端，自动故障切换 |
| **嵌入式 Embedding** | FastEmbed 本地向量嵌入，模型 intfloat/multilingual-e5-base（768 维） |

---

## 二、近期修复与优化

### 2.1 v0.5.3 记忆生命周期与去重修复

| 修复项 | 严重级别 | 影响文件 | 说明 |
|--------|---------|---------|------|
| 待办 deadline 到期失效 | 严重 | todo_list.rs, executor.rs | 原 `invalidate_expired_todos()` 未实现，deadline 到期的 pending 待办永不失效；新增方法并在心跳维护中调用，将过期 pending 标记为 `auto_invalid` |
| `auto_invalid_at` 死字段 | 严重 | contact.rs, 7个事件创建点 | `auto_invalid_at` 字段全为 NULL，事件永不自动失效；新增 `compute_auto_invalid_at()` 方法，在所有7个事件创建点调用，根据 importance_score 自动计算失效时间 |
| `cleanup_expired_events` 兜底归档未标记 | 严重 | store.rs | fallback 归档路径未设置 `is_auto_invalid=1`，导致归档事件无法区分是自动失效还是手动归档；修复为双路径均标记 |
| 记忆 promote/demote 未实现 | 严重 | store.rs, executor.rs | importance_score 创建后不可变，频繁引用的短期记忆无法提升为长期记忆；新增 `promote_demote_events()` 方法，基于事件被引用次数自动提升/降级 |
| stale 待办归档计数偏差 | 中等 | executor.rs | 原代码先插入 reminder 再归档，导致 pending 总数因 reminder 增加1而超过阈值；反转为先归档后提醒，reminder 使用归档后的最新 count |
| critical 阈值边界 bug | 中等 | contact.rs | `>= 0.95` 在 f32 精度下将 0.95 误判为 critical（永不失效），改为 `> 0.95` |
| promote 执行顺序冲突 | 中等 | executor.rs | promote 在 cleanup 之后执行，导致7天前短期记忆先被归档，promote 无法处理；移到 cleanup 之前执行 |
| promote 的 created_at 限制 | 低 | store.rs | `created_at < 7天前` 限制与 cleanup 冲突，去掉限制只看是否被近期3+事件引用 |
| analyzer 硬编码 importance | 严重 | analyzer.rs | 心跳路径硬编码 `importance_score=0.5`，即使 LLM 判定为重要(0.9)也会创建0.5分的短期记忆版本；修复为从 LLM 提取结果获取 importance |
| analyzer 无去重检查 | 严重 | analyzer.rs | 心跳路径直接 insert，不检查 background_queue 是否已创建相同事件；新增 `search_events_by_summary` 去重检查 |
| `get_conversations_since` 不过滤状态 | 严重 | dictionary/store.rs | 心跳会重新处理 background_queue 已处理的对话（status=completed）；修复为只返回 status=pending 或 failed 的对话 |

### 2.2 v0.5.2 基础修复

| 修复项 | 严重级别 | 影响文件 | 说明 |
|--------|---------|---------|------|
| UTF-8 字符串切片 panic | 严重 | analyzer.rs:668, three_library_manager.rs:491 | `&response[..200]` 在中文字符边界处 panic，改用 `chars().take(200).collect()` |
| 心跳 executing 标志卡死 | 严重 | scheduler.rs | panic 后标志未清除导致心跳永久 409，引入 `ExecutingGuard` RAII 结构 |
| 联系人匹配逻辑缺陷 | 严重 | background_queue.rs:316-424 | `search_contacts_by_name` 仅按 name 精确匹配，LLM 用别名引用时创建重复联系人；改用 `find_contact_by_any_name` 按 name OR aliases 匹配 |
| upsert 覆盖业务字段 | 严重 | background_queue.rs | `upsert_contact_index` 的 ON CONFLICT 用空值覆盖已有 background/notes/related_contact_ids/relationship_tags/relation_weight；修复为保留已有字段值 |
| batch_size 配置 | 配置 | config.toml | 从 2048 调整为 4096，解决 LLM 提取 "Insufficient Space" 错误。仅影响 GGUF LLM 推理的 n_batch 参数，不影响 embedder/向量库/SQLite |
| SELECT 字段补全 | 中等 | store.rs | 6 处 contact_index SELECT 查询缺少 is_archived/archived_at/activity_score 字段 |
| 画像 JSON 解析 | 中等 | analyzer.rs:23-64 | LLM 返回 markdown 代码块包裹的 JSON，添加 `extract_json_from_response` 函数兼容 |
| 关系字段类型兼容 | 中等 | analyzer.rs:575-661 | social_relations/potential_relations 兼容字符串数组和对象数组 |
| 别名索引表写入 | 中等 | background_queue.rs, extractor.rs | contact_alias_index 表未填充，添加别名提取和写入逻辑 |
| 热表归档改为软删除 | 中等 | store.rs:2739-2759 | 超限联系人从物理 DELETE 改为 UPDATE is_archived=1 |
| 孤立向量记忆清理 | 中等 | executor.rs | 心跳维护阶段检查 SQLite event_id 存在性，删除孤立向量记忆 |
| 虚拟角色误提取 | 中等 | analyzer.rs, background_queue.rs | 虚拟角色被 100% 误提取为真实联系人，添加虚拟角色检查逻辑 |
| confirm_merge 状态更新 | 中等 | store.rs | confirm_merge 返回成功但状态未更新为 resolved |
| MAX_ACTIVE_CONTACTS 生效 | 中等 | background_queue.rs | DataDispatcher::manage_active_contacts() 为死代码，活跃联系人限制移至 write_extraction_results |
| vector_dir 配置生效 | 低 | config.toml, settings.rs | 向量库目录路径使用配置值而非硬编码默认值 |
| contact_ids 合并逻辑 | 低 | store.rs | first() 改为 is_empty() 处理所有联系人 ID |

---

## 三、架构设计

### 3.1 三库存储架构

```
┌─────────────────────────────────────────────────────────────────┐
│                    ShouXin 三库存储架构                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ │
│  │   字典库 (Sled)   │  │  关系库 (SQLite) │  │ 向量库 (HNSW)   │ │
│  │   永久全量存档     │  │  轻量快速索引     │  │  语义搜索        │ │
│  ├─────────────────┤  ├─────────────────┤  ├─────────────────┤ │
│  │ ArchiveRecord   │  │ ContactIndex    │  │ Embedding       │ │
│  │ ContactRecord   │  │ EventIndex      │  │                 │ │
│  │ (联系人总表)     │  │ TodoItem        │  │ 768 维向量       │ │
│  │                 │  │ Conversation    │  │                 │ │
│  │ 嵌套 KV 存储     │  │ FileRecord      │  │ 支持 HNSW 和     │ │
│  │ 前缀组织         │  │                 │  │ Embedded 后端    │ │
│  │                 │  │ WAL 模式         │  │                 │ │
│  │                 │  │ 部分唯一索引      │  │                 │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘ │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

| 存储库 | 存储引擎 | 存储内容 | 特性 |
|--------|---------|---------|------|
| **字典库** | Sled | `ArchiveRecord`, `ContactRecord`（联系人总表） | 嵌套 KV 存储，前缀组织 |
| **关系库** | SQLite | `ContactIndex`, `EventIndex`, `TodoItem`, `Conversation`, `FileRecord` 等 | WAL 模式，部分唯一索引 |
| **向量库** | HNSW / Embedded | `Embedding` 向量（768 维） | 支持 HNSW 和 Embedded 后端 |

### 3.2 Embedding 模型

| 属性 | 值 |
|------|-----|
| **模型名称** | `intfloat/multilingual-e5-base` |
| **向量维度** | 768 |
| **推理引擎** | FastEmbed |
| **缓存目录** | `.fastembed_cache/` |

---

## 四、模块结构

```
src/
├── api/                      # REST API 层
│   ├── routes.rs             # HTTP 路由
│   ├── async_memory.rs       # 异步记忆 API 封装
│   ├── sync.rs               # 同步 API 封装
│   └── mod.rs
├── config/                   # 配置系统
│   ├── app.rs                # 应用配置
│   ├── embedder.rs           # 嵌入器配置
│   ├── llm.rs                # LLM 配置
│   ├── settings.rs           # 设置管理
│   ├── vector_store.rs       # 向量存储配置
│   └── mod.rs
├── core/                     # 核心数据结构
│   ├── contact.rs            # 联系人 (Contact)
│   ├── event.rs              # 事件索引 (EventIndex)
│   ├── embedding.rs          # 嵌入向量
│   ├── memory.rs             # 记忆结构
│   └── mod.rs
├── embedder/                 # 嵌入引擎
│   ├── fastembed.rs          # FastEmbed 实现
│   ├── factory.rs            # 嵌入器工厂
│   └── mod.rs
├── error/                    # 错误类型
│   └── error.rs              # ShouxinError
├── heartbeat/                # 心跳模块
│   ├── scheduler.rs          # 周期性扫描调度
│   ├── executor.rs           # 分阶段执行器
│   ├── analyzer.rs           # LLM 分析器
│   ├── question_pool.rs      # 问题生成池
│   └── mod.rs
├── initialization/           # 系统初始化
│   └── initialization.rs
├── learning/                 # 学习模块
│   ├── knowledge_graph.rs    # 知识图谱提取
│   ├── entity_normalizer.rs  # 实体标准化
│   ├── event_dedup.rs        # 事件去重
│   └── mod.rs
├── llm/                      # LLM 客户端管理
│   ├── manager.rs            # 多后端管理器
│   ├── ollama.rs             # Ollama 实现
│   ├── gguf.rs               # GGUF 实现 (llama.cpp)
│   ├── api.rs                # API 客户端 (OpenAI 兼容)
│   ├── factory.rs            # LLM 工厂
│   ├── sanitizer.rs          # LLM 输出清洗器
│   └── mod.rs
├── memory/                   # 三库记忆管理
│   ├── three_library_manager.rs  # 三库管理器
│   ├── archive_manager.rs    # 归档管理器
│   └── mod.rs
├── memory_extractor/         # LLM 记忆提取
│   ├── extractor.rs          # 提取器
│   ├── dispatcher.rs         # 数据分流
│   └── mod.rs
├── service/                  # 后台服务
│   ├── internal_api.rs       # 内部 API
│   ├── file_processor.rs     # 文件处理
│   └── mod.rs
├── stores/                   # 存储实现
│   ├── dictionary/           # 字典库 (Sled)
│   ├── relational/           # 关系库 (SQLite)
│   └── vector/               # 向量库 (HNSW/Embedded)
├── traits/                   # Trait 定义
│   ├── llm.rs                # LlmClient Trait
│   ├── embedder.rs           # Embedder Trait
│   ├── vector_store.rs       # VectorStore Trait
│   ├── store_traits.rs       # 存储层 Trait
│   └── mod.rs
├── utils/                    # 工具函数
│   └── mod.rs
├── lib.rs
└── shouxin-server.rs
```

---

## 五、关键数据结构

### 5.1 Scope 枚举

```rust
pub enum Scope {
    User,
    Agent,
    Contact,
    Public,
}
```

| 值 | 说明 |
|-----|------|
| `User` | 用户作用域 |
| `Agent` | Agent 作用域 |
| `Contact` | 联系人作用域 |
| `Public` | 公开作用域 |

### 5.2 EventIndex（v0.5.1）

```rust
struct EventIndex {
    event_id: String,
    contact_ids: Vec<String>,
    owner_id: String,
    owner_type: String,
    time_range_start: DateTime<Utc>,
    time_range_end: DateTime<Utc>,
    importance: Importance,
    status: EventStatus,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    deadline: Option<DateTime<Utc>>,
    auto_invalid_at: Option<DateTime<Utc>>,
    summary: String,
    title: Option<String>,
    event_type: String,
    is_auto_invalid: bool,
    // v0.5.1 新增
    events_merged: u32,
    contacts_merged: u32,
    ambiguities_detected: u32,
    profiles_updated: u32,
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `event_id` | `String` | 事件唯一标识 |
| `contact_ids` | `Vec<String>` | 关联联系人 ID 列表 |
| `owner_id` | `String` | 所属者 ID |
| `owner_type` | `String` | 所属者类型 |
| `time_range_start` | `DateTime<Utc>` | 时间范围起始 |
| `time_range_end` | `DateTime<Utc>` | 时间范围结束 |
| `importance` | `Importance` | 重要程度 |
| `status` | `EventStatus` | 状态 |
| `created_at` | `DateTime<Utc>` | 创建时间 |
| `completed_at` | `Option<DateTime<Utc>>` | 完成时间 |
| `deadline` | `Option<DateTime<Utc>>` | 截止时间 |
| `auto_invalid_at` | `Option<DateTime<Utc>>` | 自动失效时间 |
| `summary` | `String` | 事件摘要 |
| `title` | `Option<String>` | 事件标题 |
| `event_type` | `String` | 事件类型 |
| `is_auto_invalid` | `bool` | 是否自动失效 |
| `events_merged` | `u32` | 已合并事件数（v0.5.1 新增） |
| `contacts_merged` | `u32` | 已合并联系人数（v0.5.1 新增） |
| `ambiguities_detected` | `u32` | 检测到的歧义数（v0.5.1 新增） |
| `profiles_updated` | `u32` | 已更新画像数（v0.5.1 新增） |

### 5.3 EventDetail（API 响应）

```rust
struct EventDetail {
    event_id: String,
    title: String,
    event_type: String,
    description: String,
    time_range_start: DateTime<Utc>,
    time_range_end: DateTime<Utc>,
    importance: Importance,
    status: EventStatus,
    contact_ids: Vec<String>,
    owner_type: String,
    owner_id: String,
    deadline: Option<DateTime<Utc>>,
}
```

### 5.4 TodoItem（v0.5.1）

```rust
struct TodoItem {
    todo_id: String,
    title: String,
    description: String,
    deadline: Option<DateTime<Utc>>,
    importance: Importance,       // high / medium / low
    status: TodoStatus,           // pending / completed
    contact_ids: Vec<String>,
    source_event_id: Option<String>,
    owner_id: String,
    owner_type: String,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    // v0.5.1 新增
    archived_at: Option<DateTime<Utc>>,
    todo_type: String,
    is_archived: bool,
    last_updated_at: DateTime<Utc>,
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `todo_id` | `String` | 待办唯一标识 |
| `title` | `String` | 待办标题 |
| `description` | `String` | 待办描述 |
| `deadline` | `Option<DateTime<Utc>>` | 截止时间 |
| `importance` | `Importance` | 重要程度 |
| `status` | `TodoStatus` | 状态 |
| `contact_ids` | `Vec<String>` | 关联联系人 |
| `source_event_id` | `Option<String>` | 来源事件 |
| `owner_id` | `String` | 所属者 ID |
| `owner_type` | `String` | 所属者类型 |
| `created_at` | `DateTime<Utc>` | 创建时间 |
| `completed_at` | `Option<DateTime<Utc>>` | 完成时间 |
| `archived_at` | `Option<DateTime<Utc>>` | 归档时间（v0.5.1 新增） |
| `todo_type` | `String` | 待办类型（v0.5.1 新增） |
| `is_archived` | `bool` | 是否已归档（v0.5.1 新增） |
| `last_updated_at` | `DateTime<Utc>` | 最后更新时间（v0.5.1 新增） |

**待办唯一索引（P2-1 修复）**：

```sql
CREATE UNIQUE INDEX idx_todos_unique_active
ON todo_list(title, owner_id, owner_type, deadline)
WHERE status = 'pending';
```

### 5.5 ContactIndex

```rust
struct ContactIndex {
    id: String,
    name: String,
    aliases: Vec<String>,
    is_self: bool,
    is_agent: bool,
    created_at: DateTime<Utc>,
    last_interaction_at: Option<DateTime<Utc>>,
    related_contact_ids: Vec<String>,
    relationship_tags: Vec<String>,
    relation_weight: f32,
    background: Option<String>,
    notes: Option<String>,
    owner_type: String,
    owner_id: String,
    parent_agent_id: Option<String>,
}
```

### 5.6 联系人管理逻辑（v0.5.2）

#### 5.6.1 联系人匹配：find_contact_by_any_name

v0.5.2 修复了联系人匹配逻辑缺陷。原 `search_contacts_by_name` 仅按 `name` 字段精确匹配，当 LLM 使用别名（aliases）引用已有联系人时，系统无法识别为同一实体，从而创建重复联系人。

**修复方案**：改用 `find_contact_by_any_name`，按 `name OR aliases` 匹配：

```rust
// 匹配逻辑：name 精确匹配 OR aliases 包含匹配
fn find_contact_by_any_name(&self, name: &str, owner_id: &str) -> Option<ContactIndex> {
    // 1. 优先按 name 精确匹配
    // 2. 若未命中，遍历 aliases 检查是否包含该名称
    // 3. 命中则返回已有联系人，避免重复创建
}
```

**影响**：避免 LLM 用别名引用联系人时创建重复记录，保证联系人实体唯一性。

#### 5.6.2 upsert 保留已有业务字段

v0.5.2 修复了 `upsert_contact_index` 的 ON CONFLICT 子句用空值覆盖已有业务字段的问题。

**问题背景**：当 LLM 提取的联系人数据 upsert 到 `contact_index` 表时，ON CONFLICT 更新子句使用新值覆盖所有字段。若新提取的数据中某些字段为空（LLM 未提取到），则会用空值覆盖已有数据。

**受影响字段**：

| 字段 | 说明 |
|------|------|
| `background` | 联系人背景信息 |
| `notes` | 联系人备注 |
| `related_contact_ids` | 关联联系人 ID 列表 |
| `relationship_tags` | 关系标签 |
| `relation_weight` | 关系权重 |

**修复方案**：ON CONFLICT 更新时，使用 `COALESCE` 或条件判断保留已有字段值，仅在新值非空时更新：

```sql
-- 修复前（会用空值覆盖）
ON CONFLICT(id) DO UPDATE SET
    background = excluded.background,
    notes = excluded.notes,
    ...

-- 修复后（保留已有值）
ON CONFLICT(id) DO UPDATE SET
    background = COALESCE(NULLIF(excluded.background, ''), contact_index.background),
    notes = COALESCE(NULLIF(excluded.notes, ''), contact_index.notes),
    ...
```

#### 5.6.3 热表归档：软删除

v0.5.2 将超限联系人的归档方式从物理 DELETE 改为软删除（`is_archived = 1`）。

**问题背景**：当活跃联系人数量超过 `MAX_ACTIVE_CONTACTS` 限制时，原实现使用 `DELETE FROM contact_index` 物理删除超限联系人。这导致：
- 数据不可恢复
- 关联事件/待办的 `contact_ids` 出现悬空引用
- 字典库（Sled）中的 `ContactRecord` 与关系库（SQLite）数据不一致

**修复方案**：改为 `UPDATE contact_index SET is_archived = 1, archived_at = ? WHERE ...`，保留数据记录，仅在查询时过滤已归档联系人。

**影响**：
- 归档联系人可通过 `is_archived = 0` 恢复
- 关联数据完整性得到保障
- 查询活跃联系人时增加 `WHERE is_archived = 0` 过滤条件

---

## 六、LLM 系统

### 6.1 多后端架构

```
┌─────────────────────────────────────────────────────────────┐
│                   LlmClientManager                           │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  primary: Box<dyn LlmClient>    (gguf)                │  │
│  │  secondary: Option<Box<dyn LlmClient>>  (ollama/api)  │  │
│  └───────────────────────────────────────────────────────┘  │
│                          │                                   │
│          ┌───────────────┼───────────────┐                  │
│          ▼               ▼               ▼                  │
│   ┌──────────┐   ┌──────────┐   ┌──────────┐              │
│   │   GGUF   │   │  Ollama  │   │   API    │              │
│   │  Client  │   │  Client  │   │  Client  │              │
│   └──────────┘   └──────────┘   └──────────┘              │
└─────────────────────────────────────────────────────────────┘
```

| 后端 | 配置值 | 说明 |
|------|--------|------|
| **GGUF** | `gguf` | 主后端，内置 llama.cpp 加载 .gguf 模型文件 |
| **Ollama** | `ollama` | 可选备后端，连接本地 Ollama 服务 |
| **API** | `api` | 云 API 后端，OpenAI 兼容接口 |

**默认配置**：
- 主后端：`gguf`（Llama-3.2-3B-Instruct-Q5_K_M.gguf，99 GPU 层）
- 备后端：`ollama`（可选），`api`（云 API）
- 故障时自动切换，可配置重试

### 6.2 记忆提取流水线（v0.5.1）

```
输入（对话文本）
     │
     ▼
┌──────────────────┐
│  长文本截断       │  ← P0-1: 超长输入截断，防止 LLM 崩溃
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│  LLM 提示词       │  ← 结构化 JSON 输出格式
│  (JSON 格式输出)  │
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│ LlmOutputSanitizer│  ← P1: 乱码输出清洗、字符集检测、JSON 片段提取
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│  解析 JSON 响应   │
└──────┬───────────┘
       │
   ┌───┴───┐
   │ 成功？  │
   └───┬───┘
  Yes   │   No
       │    └──▶ ┌──────────────────┐
       │        │  降级正则提取     │  ← P0-2: LLM 失败/输出异常时降级
       │        └──────┬───────────┘
       │               │
       ▼               ▼
┌──────────────────────────────┐
│  后处理                       │
│  · 事件去重（按事件描述）      │
│  · 联系人去重（按名称）        │
│  · 待办归档检查                │
│  · 提醒生成判断                │
└──────────────────────────────┘
```

### 6.3 LlmOutputSanitizer（P1 新增）

当 LLM 返回乱码或异常输出时，`LlmOutputSanitizer` 负责：

| 功能 | 说明 |
|------|------|
| **字符集检测** | 检测输出编码是否合法 UTF-8 |
| **乱码过滤** | 去除不可打印字符、乱码片段 |
| **JSON 片段提取** | 从混合输出中提取有效 JSON 块 |
| **格式修复** | 修复截断 JSON、补全缺失括号 |

### 6.4 联系人提取（P1-1 增强）

**职衔词识别**：

| 类别 | 词汇 |
|------|------|
| 职衔词 | 总监、工程师、组长、经理、主任、秘书、主管、工、总、博士 |
| 复合姓氏 | 欧阳、诸葛、司马、上官、夏侯、慕容、皇甫、尉迟、长孙 |
| 昵称模式 | 小X、老X |

**提取策略**：
- 从对话上下文中动态检测姓氏
- 复合姓氏优先匹配
- 昵称模式正则识别
- 后处理：去重、标准化、过滤噪声

### 6.5 事件提取（P1-2 增强）

- 独立事件标题（非完整对话文本）
- 按事件描述去重（非共享对话去重）
- 长描述关键字级别事件拆分
- 从事件描述中提取标题

---

## 七、配置系统

### 7.1 配置文件 (config.toml)

```toml
[llm]
primary = "gguf"
secondary = "none"
temperature = 0.7
max_tokens = 2048
timeout_secs = 600

[llm.gguf]
model_path = "./models/Llama-3.2-3B-Instruct-Q5_K_M.gguf"
context_size = 8192
threads = 4
# batch_size 仅影响 GGUF LLM 推理的 prompt 批处理大小（n_batch 参数），不影响 embedder/向量库/SQLite
batch_size = 4096
n_gpu_layers = 99
offload_kv_cache = false

[llm.ollama]
model = "llama3.2:3b"
endpoint = "http://localhost:11434"
timeout_secs = 120
max_retries = 2

[llm.api]
endpoint = "https://api.openai.com/v1"
api_key = ""
model = "gpt-3.5-turbo"
timeout_secs = 60
max_retries = 2

[llm.heartbeat]
backend = "gguf"
model = "./models/Llama-3.2-3B-Instruct-Q5_K_M.gguf"
endpoint = "gguf://localhost"
temperature = 0.3
max_tokens = 500

[embedder]
model_path = ".fastembed_cache"

[heartbeat]
enabled = true
base_interval_secs = 60
min_interval_secs = 30
max_interval_secs = 120
adaptive = false
idle_threshold_secs = 30

[storage]
data_dir = "./data"
dictionary_dir = "dictionary"
relational_dir = "relational"
vector_dir = "vector"
relational_db_file = "relational.db"
vector_index_file = "vectors.hnsw"
question_pool_dir = "heartbeat/questions"

[logging]
level = "info"
dir = "./logs"
max_file_size_mb = 10

[agent]
primary_agent_id = "primary_agent_default"
internal_api_token = "shouxin-v050-gguf-test-token"

[server]
host = "127.0.0.1"
port = 3000
internal_api_token = "shouxin-v050-gguf-test-token"

[event]
long_term_hot_limit = 200
short_term_hot_limit = 100
```

### 7.2 环境变量覆盖

| 环境变量 | 示例值 | 说明 |
|----------|--------|------|
| `SHOUXIN_LLM__PRIMARY` | `gguf` | LLM 主后端 |
| `SHOUXIN_STORE_TYPE` | `hnsw` | 向量存储类型 |
| `HF_HUB_OFFLINE` | `1` | HuggingFace 离线模式 |
| `FASTEMBED_CACHE_DIR` | `.fastembed_cache` | 嵌入模型缓存目录 |

---

## 八、心跳模块（v0.5.1 拆分架构）

### 8.1 拆分式 HeartbeatExecutor

v0.5.1 将心跳执行拆分为两个独立阶段：

```
HeartbeatScheduler (定时调度)
  → HeartbeatExecutor (分阶段执行器)
    Phase 1: 维护阶段（快速）
      · 清理过期联系人
      · 事件去重
      · 联系人合并
      · 歧义检测
      · 孤立向量记忆清理（检查 SQLite event_id 存在性，删除向量库中对应 event_id 已不存在的孤立记忆）
    Phase 2: LLM 阶段（慢速）
      · 画像分析
      · 问题生成
```

### 8.2 关键组件

| 组件 | 说明 |
|------|------|
| **HeartbeatScheduler** | 定时调度，带超时控制 |
| **HeartbeatExecutor** | 分两阶段执行，维护优先 |
| **HeartbeatAnalyzer** | LLM 驱动，仅第二阶段调用 |
| **QuestionPool** | 问题池持久化 |
| **ExecutingGuard** | RAII 结构，确保心跳 executing 标志在 panic 后也能被清除（v0.5.2 新增） |

### 8.3 HeartbeatReport

```rust
struct HeartbeatReport {
    maintenance_only: bool,       // 仅执行维护阶段
    events_merged: u32,           // 合并事件数
    contacts_merged: u32,         // 合并联系人数
    ambiguities_detected: u32,    // 检测歧义数
    profiles_updated: u32,        // 更新画像数
}
```

### 8.4 HeartbeatAction 枚举

```rust
pub enum HeartbeatAction {
    AskUser,
    UpdateContactSummary,
    MarkEventCompleted,
    MergeEntities,
    CleanupPendingEvents,
    UpdateTodoStatus,
    ResolveAmbiguity,
    NoOp,
}
```

### 8.5 ProcessingState

```rust
struct ProcessingState {
    heartbeat_count: u64,         // 累计心跳次数
    consecutive_skips: u64,       // 连续跳过次数
}
```

### 8.6 ExecutingGuard（v0.5.2 新增）

v0.5.2 引入 `ExecutingGuard` RAII 结构，解决心跳 `executing` 标志在 panic 后未清除导致心跳永久返回 409 的问题。

**问题背景**：
- `HeartbeatScheduler` 使用 `executing` 标志防止心跳并发执行
- 若心跳执行过程中发生 panic，标志未清除，后续所有心跳请求均被拒绝（HTTP 409）
- 系统进入"心跳卡死"状态，无法自愈

**解决方案**：

```rust
struct ExecutingGuard {
    flag: Arc<AtomicBool>,
}

impl ExecutingGuard {
    fn new(flag: Arc<AtomicBool>) -> Option<Self> {
        // CAS 设置标志，失败返回 None（已有心跳在执行）
        if flag.compare_exchange(false, true).is_ok() {
            Some(Self { flag })
        } else {
            None
        }
    }
}

impl Drop for ExecutingGuard {
    fn drop(&mut self) {
        // 无论正常返回还是 panic，Drop 都会执行，清除标志
        self.flag.store(false, Ordering::SeqCst);
    }
}
```

**效果**：`ExecutingGuard` 在作用域结束时（包括 panic 展开）自动清除 `executing` 标志，确保心跳不会因 panic 而永久卡死。

### 8.7 孤立向量记忆清理（v0.5.2 新增）

v0.5.2 在心跳维护阶段新增孤立向量记忆清理逻辑。

**问题背景**：
- 事件被删除或合并后，向量库中对应的记忆向量可能未被同步清理
- 长期累积导致向量库存在"孤立记忆"，影响语义搜索质量

**清理流程**：
1. 扫描向量库中的所有记忆向量
2. 对每个向量记录的 `event_id`，检查 SQLite `event_index` 表中是否存在
3. 若 `event_id` 在 SQLite 中已不存在，则从向量库删除该孤立记忆

该清理在 Phase 1 维护阶段执行，不依赖 LLM，开销较低。

### 8.8 记忆生命周期管理（v0.5.3 新增）

v0.5.3 实现了完整的记忆生命周期管理，包括自动失效、提升/降级和待办到期处理。

#### 8.8.1 事件自动失效 (auto_invalid_at)

事件创建时根据 `importance_score` 自动计算失效时间：

| 重要性范围 | 记忆类型 | 失效时间 | 说明 |
|-----------|---------|---------|------|
| `> 0.95` | critical | 永不失效 (None) | 关键记忆，如合同签署、重大事件 |
| `> 0.7` | 长期记忆 | 30天后 | 重要事件，如会议、项目讨论 |
| `0.4 - 0.7` | 短期记忆 | 7天后 | 普通事件，如日常沟通 |
| `<= 0.4` | 低重要性 | 3天后 | 低价值信息，快速遗忘 |

**实现**：`EventIndex::compute_auto_invalid_at()` 方法在所有7个事件创建点调用。

#### 8.8.2 心跳维护阶段执行顺序（v0.5.3）

```
Phase 1: 维护阶段（v0.5.3 更新）
  1. cleanup_expired_pending_merges     # 清理过期合并请求
  2. promote_demote_events              # 记忆提升/降级（必须在cleanup之前）
  3. cleanup_expired_events             # 过期事件归档（双路径：auto_invalid_at + fallback）
  4. cleanup_low_importance_events      # 低重要性事件删除 (importance < 0.4)
  5. cleanup_orphaned_vector_memories   # 孤立向量记忆清理
  6. archive_stale_contacts             # 联系人归档
  7. invalidate_expired_todos           # 待办deadline到期失效
  8. cleanup_todos                      # 待办归档
  9. Todo hot/cold management           # 先归档(>50)后提醒(>30)
```

#### 8.8.3 记忆提升/降级 (promote_demote_events)

基于事件在近期对话中被引用的次数调整 `importance_score`：

| 操作 | 条件 | 效果 |
|------|------|------|
| **Promote** | 短期记忆(0.4-0.7)在最近7天被3+个新事件共享 contact_ids | `importance_score` 提升至 0.75（变为长期记忆） |
| **Demote** | 长期记忆(>0.7, <0.95)在最近60天无任何新事件共享 contact_ids | `importance_score` 降至 0.65（变为短期记忆） |

**执行顺序**：必须在 `cleanup_expired_events` 之前执行，否则被频繁引用的短期记忆会被先归档。

#### 8.8.4 过期事件双路径归档 (cleanup_expired_events)

```
路径1: 基于 auto_invalid_at 的精确失效
  UPDATE event_index SET is_archived=1, archived_at=?, is_auto_invalid=1
  WHERE is_archived=0 AND auto_invalid_at IS NOT NULL AND auto_invalid_at < ?

路径2: 基于 importance_score + created_at 的兜底归档（兼容旧数据）
  UPDATE event_index SET is_archived=1, archived_at=?, is_auto_invalid=1
  WHERE is_archived=0 AND (
    (importance_score > 0.7 AND created_at < 30天前)
    OR (importance_score <= 0.7 AND importance_score > 0.4 AND created_at < 7天前)
  )
```

#### 8.8.5 待办 deadline 到期失效 (invalidate_expired_todos)

```sql
UPDATE todo_list SET status='auto_invalid', last_updated_at=?
WHERE status='pending' AND is_archived=0 AND deadline < ?
```

将 deadline 已过期但仍为 pending 的待办标记为 `auto_invalid`，避免过期待办堆积。

### 8.9 事件去重三重防护（v0.5.3 新增）

v0.5.3 实现了三层防护机制，确保同一事件不会被重复创建：

| 防护层 | 位置 | 机制 |
|--------|------|------|
| **Layer 1** | background_queue.rs | `search_events_by_summary` 查重，已存在则跳过 |
| **Layer 2** | analyzer.rs | 心跳路径 insert 前调用 `search_events_by_summary` 查重 |
| **Layer 3** | dictionary/store.rs | `get_conversations_since` 只返回 status=pending/failed 的对话，跳过已处理对话 |
| **DB 约束** | store.rs | `idx_unique_event_summary` 唯一索引 + `INSERT OR IGNORE` 兜底 |

### 8.10 LLM 重要性传递（v0.5.3 新增）

v0.5.3 修复了心跳路径硬编码 `importance_score=0.5` 的问题：

- LLM prompt 新增 `importance` 字段（0.0-1.0浮点数）
- 解析时从 LLM 输出获取 importance，clamp 到 0-1 范围
- `process_extracted_event` 使用 LLM 的 importance 而非硬编码 0.5
- importance_label 也根据 LM 的 importance 动态生成（high/medium/low）

---

## 九、编译特性

| Feature | 说明 |
|---------|------|
| `gguf` | 启用 llama-cpp-2 用于 GGUF 模型推理 |
| `gguf-cuda` | 与 gguf 相同（CUDA 支持） |

```bash
cargo build --release --features gguf
```

---

## 十、数据库

### 10.1 SQLite 配置

| 参数 | 值 |
|------|-----|
| Journal 模式 | WAL |
| 编码 | UTF-8 |
| busy_timeout | 5000 ms |
| synchronous | NORMAL |

### 10.2 数据表

| 表名 | 说明 |
|------|------|
| `contact_index` | 联系人索引 |
| `event_index` | 事件索引 |
| `pending_event_merge` | 待合并事件 |
| `todo_list` | 待办列表 |
| `entity_alias` | 实体别名 |
| `slice_relation` | 切片关系 |
| `slice_keyword` | 切片关键词 |
| `schema_migrations` | 数据库迁移记录 |
| `memory_processing_state` | 记忆处理状态 |
| `contact_profile_update_log` | 联系人画像更新日志 |
| `contact_merge_log` | 联系人合并日志 |
| `event_merge_log` | 事件合并日志 |
| `ambiguity_log` | 歧义日志 |
| `contact_alias_index` | 联系人别名索引 |
| `virtual_character_aliases` | 虚拟角色别名 |
| `file_index` | 文件索引 |
| `file_relations` | 文件关联 |

---

## 十一、v0.5.1 关键修复

| 编号 | 类别 | 修复内容 |
|------|------|---------|
| **P0-1** | 稳定性 | 长对话输入截断 + Worker 鲁棒性增强，防止 LLM 崩溃 |
| **P0-2** | 容错 | LLM 输出异常时降级正则表达式提取 |
| **P0-3** | 架构 | 心跳拆分：维护阶段与 LLM 阶段分离，新增 `maintenance_only` 字段 |
| **P1-1** | 联系人提取 | 增强覆盖率：职衔词、复合姓氏、昵称模式识别 |
| **P1-2** | 事件存储 | 独立事件存储，带标题（不复用完整对话文本） |
| **P1-3** | LLM 输出 | `LlmOutputSanitizer` 乱码输出清洗、字符集检测、JSON 片段提取 |
| **P2-1** | 待办去重 | 放宽去重条件，允许相同标题不同截止时间的待办 |
| **P2-2** | 归档 | 写入路径自动归档：`pending > 50` 时触发 `archive_stale_todos`，仅归档 >30 天过期待办 |
| **P2-3** | 提醒 | 写入路径自动提醒：`pending > 30` 时触发 `insert_reminder_todo` |
| **P3-1** | 文件上传 | 异步化：DB 插入同步确保即时元数据返回，文本提取通过 `tokio::spawn` 异步处理 |

### 11.1 P0-1：长对话输入截断

超长对话文本在送入 LLM 前进行截断处理，避免超出上下文窗口导致 LLM 崩溃。Worker 层面增加鲁棒性保护。

### 11.2 P0-2：降级正则提取

当 LLM 返回异常或 JSON 解析失败时，系统自动降级为基于正则表达式的结构化提取，确保基础功能可用。

### 11.3 P0-3：心跳拆分架构

v0.5.1 将心跳执行拆分为维护阶段（快速：清理过期联系人、事件去重、联系人合并、歧义检测）和 LLM 阶段（慢速：画像分析、问题生成）。新增 `maintenance_only` 字段支持仅执行维护的轻量心跳。

### 11.4 P1-1：联系人提取增强

新增三类提取模式：

| 模式 | 示例 |
|------|------|
| 职衔词识别 | 王总监、李工程师、张经理 |
| 复合姓氏 | 欧阳锋、诸葛明、司马光 |
| 昵称模式 | 小王、老李 |

### 11.5 P1-2：事件独立存储

事件不再以完整对话文本作为存储内容，改为独立的事件标题和描述。

### 11.6 P1-3：LLM 输出清洗

`LlmOutputSanitizer` 在 JSON 解析前对 LLM 原始输出进行清洗——检测字符集合法性、过滤乱码、提取有效 JSON 片段——大幅提升提取成功率。

### 11.7 P2-1：待办去重放宽

```sql
CREATE UNIQUE INDEX idx_todos_unique_active
ON todo_list(title, owner_id, owner_type, deadline)
WHERE status = 'pending';
```

允许同一用户创建标题相同但截止时间不同的多个待办事项。

### 11.8 P2-2：写入路径自动归档

每次写入操作后检查 pending 待办数量：若 `pending > 50`，触发 `archive_stale_todos`，仅对创建时间超过 30 天的 pending 待办执行归档。

### 11.9 P2-3：写入路径自动提醒

每次写入操作后检查 pending 待办数量：若 `pending > 30`，触发 `insert_reminder_todo` 自动生成提醒待办。

### 11.10 P3-1：文件上传异步化

文件上传时 DB 插入同步执行以确保立即返回元数据，文本提取（可能需要 OCR/解析）通过 `tokio::spawn` 异步化，不阻塞 API 响应。

---

## 十二、API 端点概览

### 12.1 文件端点

| 端点 | 方法 | 认证 | 说明 |
|------|------|------|------|
| `/api/files/upload` | POST | 无 | 上传文件 |
| `/api/files/:file_id` | GET | 无 | 获取文件元数据 |
| `/api/files/:file_id/text` | GET | 无 | 获取文件文本内容 |
| `/api/files/:file_id` | DELETE | 无 | 删除文件 |

### 12.2 记忆端点

| 端点 | 方法 | 认证 | 说明 |
|------|------|------|------|
| `/api/memory/add` | POST | 无 | 添加对话并提取四要素 |
| `/api/memory/extract_status/:conversation_id` | GET | 无 | 获取提取状态 |
| `/api/memory/search/events` | POST | 无 | 搜索事件 |
| `/api/memory/search/history` | POST | 无 | 搜索历史对话 |
| `/api/memory/search/contacts` | POST | 无 | 搜索联系人 |
| `/api/memory/search/semantic` | POST | 无 | 语义搜索 |

### 12.3 事件端点

| 端点 | 方法 | 认证 | 说明 |
|------|------|------|------|
| `/api/events/merge` | GET | 无 | 获取待合并事件 |
| `/api/events/confirm_merge` | POST | 无 | 确认事件合并 |

### 12.4 向量端点

| 端点 | 方法 | 认证 | 说明 |
|------|------|------|------|
| `/api/vector/search` | POST | 无 | 向量语义搜索 |
| `/api/vector/:id` | GET | 无 | 获取学习素材详情 |

### 12.5 待办端点

| 端点 | 方法 | 认证 | 说明 |
|------|------|------|------|
| `/api/todos` | POST | 无 | 添加待办事项 |
| `/api/todos` | GET | 无 | 获取待办列表 |
| `/api/todos/:id` | GET | 无 | 获取待办详情 |
| `/api/todos/:id/complete` | PUT | 无 | 完成待办 |
| `/api/todos/:id` | DELETE | 无 | 删除待办 |

### 12.6 心跳端点

| 端点 | 方法 | 认证 | 说明 |
|------|------|------|------|
| `/api/heartbeat/trigger` | POST | 无 | 触发心跳 |
| `/api/heartbeat/pending_questions` | GET | 无 | 获取待确认问题 |
| `/api/heartbeat/status` | GET | 无 | 获取心跳状态 |
| `/api/heartbeat/answer` | POST | 无 | 回答问题 |

### 12.7 搜索端点

| 端点 | 方法 | 认证 | 说明 |
|------|------|------|------|
| `/api/search/smart` | POST | 无 | 智能搜索 |

### 12.8 虚拟实体端点

| 端点 | 方法 | 认证 | 说明 |
|------|------|------|------|
| `/api/virtual_entities` | GET | 无 | 列出虚拟实体 |
| `/api/virtual_entities/:id` | GET | 无 | 获取虚拟实体详情 |
| `/api/virtual_entities` | POST | 无 | 添加虚拟实体 |
| `/api/virtual_entities/:id` | PUT | 无 | 更新虚拟实体 |
| `/api/virtual_entities/:id` | DELETE | 无 | 删除虚拟实体 |
| `/api/virtual_entities/check/:name` | GET | 无 | 检查虚拟实体匹配 |

### 12.9 系统端点

| 端点 | 方法 | 认证 | 说明 |
|------|------|------|------|
| `/health` | GET | 无 | 健康检查 |
| `/internal/...` | - | 内部 | 内部 API |

---

## 十三、相关资源

| 资源 | 链接 |
|------|------|
| Rust 官网 | https://www.rust-lang.org |
| Cargo 文档 | https://doc.rust-lang.org/cargo |
| Tokio 文档 | https://tokio.rs |
| FastEmbed | https://github.com/Anush008/fastembed-rs |
| llama.cpp | https://github.com/ggerganov/llama.cpp |
| GGUF 模型下载 | https://huggingface.co/models?other=gguf |
| Axum 文档 | https://docs.rs/axum |
| sled | https://github.com/spacejam/sled |
| SQLx | https://github.com/launchbadge/sqlx |
| Ollama | https://ollama.ai |
| intfloat/multilingual-e5-base | https://huggingface.co/intfloat/multilingual-e5-base |

---

*文档版本: v0.5.2*
*最后更新: 2026-07-15*