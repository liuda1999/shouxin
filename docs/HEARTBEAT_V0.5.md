# ShouXin v0.5.1 心跳模块文档

## 概述

心跳模块是 ShouXin 的主动式信息维护系统，通过周期性扫描用户数据，执行维护任务并生成待处理问题。v0.5.1 引入拆分式架构，将快速维护操作与慢速 LLM 分析解耦。

## 拆分架构（v0.5.1）

```
HeartbeatScheduler (定时调度，带超时控制)
  → HeartbeatExecutor (分阶段执行器)
    Phase 1: 维护阶段（快速，无 LLM）
      · 清理过期联系人
      · 事件去重
      · 联系人合并
      · 歧义检测
    Phase 2: LLM 阶段（慢速，LLM 驱动）
      · 画像分析
      · 问题生成
      → QuestionPool (问题池，持久化)
```

## 核心组件

### 1. HeartbeatScheduler (调度器)

- **配置**: `SchedulerConfig { base_interval_secs: 360, min_interval_secs: 300, max_interval_secs: 600, adaptive: false, idle_threshold_secs: 30 }`
- **功能**: 定时触发心跳扫描，带超时控制
- **超时机制**: 每次心跳设置执行超时，超时后自动终止并记录
- **启动**: `heartbeat_scheduler.spawn()` 在服务器启动时调用

### 2. HeartbeatExecutor (分阶段执行器)

分两阶段执行：

#### Phase 1: 维护阶段
- **触发条件**: 每次心跳均执行
- **操作**:
  - 清理过期联系人（`cleanup_stale_contacts`）
  - 事件去重（`event_dedup`）
  - 联系人合并（`contact_merge`）
  - 歧义检测（`ambiguity_detection`）
- **特点**: 不依赖 LLM，快速执行，总耗时 < 5s

#### Phase 2: LLM 阶段
- **触发条件**: 维护阶段完成后，且非 `maintenance_only` 模式
- **操作**:
  - 联系人画像分析（`profile_analysis`）
  - 问题生成（`question_generation`）
- **特点**: 依赖 LLM，耗时较长（10s ~ 60s）

### 3. maintenance_only 模式

当心跳报告标记 `maintenance_only: true` 时，仅执行 Phase 1（维护阶段），跳过 LLM 阶段。适用于：
- 系统高负载期间
- LLM 不可用时
- 快速清理任务调度

### 4. HeartbeatReport（v0.5.1 新增字段）

```rust
struct HeartbeatReport {
    maintenance_only: bool,       // 是否仅执行维护阶段
    events_merged: u32,           // 本次心跳合并的事件数
    contacts_merged: u32,         // 本次心跳合并的联系人数
    ambiguities_detected: u32,    // 本次心跳检测到的歧义数
    profiles_updated: u32,        // 本次心跳更新的画像数
}
```

### 5. HeartbeatAction 枚举（v0.5.1 新增）

```rust
pub enum HeartbeatAction {
    AskUser,                   // 向用户提问
    UpdateContactSummary,      // 更新联系人摘要
    MarkEventCompleted,        // 标记事件完成
    MergeEntities,             // 合并实体
    CleanupPendingEvents,      // 清理待处理事件
    UpdateTodoStatus,          // 更新待办状态
    ResolveAmbiguity,          // 解决歧义
    NoOp,                      // 无操作
}
```

### 6. ProcessingState（v0.5.1 新增）

```rust
struct ProcessingState {
    heartbeat_count: u64,         // 累计心跳次数
    consecutive_skips: u64,       // 连续跳过次数
}
```

### 7. HeartbeatAnalyzer (LLM 分析器)

- 仅在 Phase 2 调用
- 使用 LLM 分析用户数据
- 检测信息缺口：联系人缺失、事件状态不明、待办遗漏等
- 生成结构化问题

### 8. QuestionPool (问题池)

- 持久化存储（SQLite + 文件）
- 存储路径: `.heartbeat_data/`
- 支持问题状态管理: pending → answered → resolved

## API 端点

### POST /api/heartbeat/trigger
手动触发一次心跳扫描。可传入 `maintenance_only` 参数仅执行维护阶段。

**请求体（可选）**：
```json
{
  "maintenance_only": true
}
```

### GET /api/heartbeat/pending_questions
获取当前待处理的问题列表。

**响应示例**：
```json
{
  "questions": [
    {
      "question_id": "uuid",
      "question": "您上次提到和王经理开会，会议内容是什么？",
      "status": "pending",
      "created_at": "2026-06-05T10:00:00Z"
    }
  ]
}
```

### GET /api/heartbeat/status
获取心跳模块运行状态：

**响应示例**：
```json
{
  "is_running": true,
  "heartbeat_count": 42,
  "consecutive_skips": 0,
  "pending_questions_count": 3,
  "maintenance_only": false,
  "events_merged": 15,
  "contacts_merged": 3,
  "ambiguities_detected": 1,
  "profiles_updated": 5
}
```

状态字段说明：

| 字段 | 类型 | 说明 |
|------|------|------|
| `is_running` | `bool` | 调度器是否运行中 |
| `heartbeat_count` | `u64` | 累计心跳次数 |
| `consecutive_skips` | `u64` | 连续跳过次数 |
| `pending_questions_count` | `u64` | 待处理问题数 |
| `maintenance_only` | `bool` | 当前是否仅维护模式 |
| `events_merged` | `u32` | 累计合并事件数 |
| `contacts_merged` | `u32` | 累计合并联系人数 |
| `ambiguities_detected` | `u32` | 累计检测歧义数 |
| `profiles_updated` | `u32` | 累计更新画像数 |

### POST /api/heartbeat/answer
回答一个问题：
```json
{
  "question_id": "uuid",
  "answer": "用户的回答"
}
```

## 配置（v0.5.1 更新）

```toml
[heartbeat]
enabled = true               # 是否启用心跳模块
base_interval_secs = 360     # 基础扫描间隔（默认值 360）
min_interval_secs = 300      # 最小间隔（300秒）
max_interval_secs = 600      # 最大间隔（600秒）
adaptive = false             # 是否自适应调整（v0.5.1 关闭自适应用于测试）
idle_threshold_secs = 30     # 空闲阈值（30秒）

> **注意**: 此处 HeartbeatSettings 配置（config.toml 中的 [heartbeat] 节）当前未被实际使用，调度器使用 `SchedulerConfig::default()` 的硬编码值（base=360, min=300, max=600, adaptive=false）。这是一个已知问题，待修复后配置将生效。

[llm.heartbeat]
backend = "gguf"             # 心跳模块使用的 LLM 后端
model = "./models/Llama-3.2-3B-Instruct-Q5_K_M.gguf"
endpoint = "gguf://localhost"
temperature = 0.3
max_tokens = 500
```

## 启动流程

在 `shouxin-server.rs` 中：

```rust
let question_pool = Arc::new(QuestionPool::new(".heartbeat_data", rel_for_heartbeat)?);
let analyzer = Arc::new(HeartbeatAnalyzer::new(llm_client, dictionary, relational));
let executor = Arc::new(HeartbeatExecutor::new(analyzer, question_pool.clone(), relational));
let scheduler = HeartbeatScheduler::new(executor, question_pool, scheduler_config);
let _heartbeat_handle = scheduler.spawn();
```

## 状态说明

| 状态值 | 含义 |
|--------|------|
| `is_running: true` | 调度器正常运行 |
| `heartbeat_count: N` | 已执行 N 次心跳 |
| `consecutive_skips: 0` | 无跳过，正常扫描 |
| `consecutive_skips > 5` | 可能空闲或异常 |
| `pending_questions_count: 0` | 无需处理的问题 |
| `maintenance_only: true` | 当前仅维护阶段，跳过 LLM 分析 |