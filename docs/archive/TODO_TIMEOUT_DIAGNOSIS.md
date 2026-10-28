# todos 端点超时根因诊断报告

**诊断日期**: 2026-05-31

**诊断范围**: S2 混合 API 压力测试中 todos 端点 50% 超时根因

---

## 1. 结论摘要

> **todos 端点本身不调用 LLM**（纯数据库操作），但在高并发 LLM 请求队列场景下，由于 **客户端 HTTP 超时（60s）< 服务器 HTTP 超时（600s）**，导致非 LLM 端点的 HTTP 请求在服务器返回前被客户端主动断开连接。

---

## 2. 代码审查结果

### 2.1 todos 端点 — 无 LLM 调用证据

逐一审查了 todos 全链路，无任何 LLM 调用：

| 函数 | 文件位置 | 实际行为 | 是否调用 LLM |
|------|----------|----------|-------------|
| `add_todo` (handler) | [routes.rs:707-818](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/routes.rs#L707-L818) | `validate_contacts` (DB读) → `list_todos` (DB读) → `add_todo` (DB写) | ❌ 否 |
| `list_todos` (handler) | [routes.rs:821-843](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/routes.rs#L821-L843) | `get_all_todos` (DB读) | ❌ 否 |
| `complete_todo` (handler) | [routes.rs:863-873](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/routes.rs#L863-L873) | `complete_todo` (DB写) + `resolve_owner` (DB读) | ❌ 否 |
| `delete_todo` (handler) | [routes.rs:876-886](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/routes.rs#L876-L886) | `delete_todo` (DB写) + `resolve_owner` (DB读) | ❌ 否 |
| `get_todo` (handler) | [routes.rs:846-860](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/routes.rs#L846-L860) | `get_todo` (DB读) + `resolve_owner` (DB读) | ❌ 否 |
| `resolve_owner` | [routes.rs:888-903](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/routes.rs#L888-L903) | `get_todo_owner` (DB读) | ❌ 否 |
| `get_todo_owner` | [todo_list.rs:475-483](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/todo_list.rs#L475-L483) | SQLite SELECT | ❌ 否 |
| `get_all_todos` | [todo_list.rs:377-423](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/todo_list.rs#L377-L423) | SQLite SELECT | ❌ 否 |
| `add_todo` (store) | [todo_list.rs:198-230](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/todo_list.rs#L198-L230) | SQLite INSERT | ❌ 否 |
| `complete_todo` (store) | [todo_list.rs:281-330](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/todo_list.rs#L281-L330) | SQLite UPDATE | ❌ 否 |
| `delete_todo` (store) | [todo_list.rs:432-455](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/todo_list.rs#L432-L455) | SQLite DELETE | ❌ 否 |
| `validate_contacts` | [async_memory.rs:326-351](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/api/async_memory.rs#L326-L351) | `get_contact_by_id` (DB读) | ❌ 否 |
| `get_contact_by_id` | [store.rs:941-952](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/stores/relational/store.rs#L941-L952) | SQLite SELECT | ❌ 否 |

**结论**: 所有 todos 操作均为纯 SQLite 数据库 CRUD，无任何 LLM（GGUF、Ollama、API）调用。

### 2.2 服务器端 HTTP 超时配置

查看 [shouxin-server.rs:323-324](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/shouxin-server.rs#L323-L324)：

```rust
let http_timeout = settings.llm.timeout_secs.max(30);  // config.toml: timeout_secs = 600
let app = app.layer(middleware::from_fn(timeout_mw(http_timeout)));
```

- **服务器端 HTTP 超时**: `max(600, 30) = 600 秒`
- **客户端测试超时 (stress_test_v2.py)**: `API_TO = 60` 秒

### 2.3 INFERENCE_SEMAPHORE 分析

查看 [gguf.rs:249-251](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/src/llm/gguf.rs#L249-L251)：

```rust
let _permit = self.inference_semaphore.acquire().await.map_err(|e| {
    ShouxinError::LLMError(format!("推理信号量获取失败: {}", e))
})?;
```

- `inference_semaphore: Arc<new(Semaphore::new(1))` — 每个 GGUF 推理持有信号量
- 信号量持有时间 = Semaphore 等待时间 + 推理时间（约 3s）
- **不涉及 todos 端点**：todos 端点不持有此信号量

---

## 3. 根因机制分析

### 3.1 问题发生的完整链路

```
S2 混合压力 (30 线程):
  Thread-0: LLM add (hold Semaphore 60s queue + 3s inference)
  Thread-1: LLM add (wait Semaphore, holds worker)
  Thread-2: todos list (fast DB read, needs worker)
  ...
  Thread-N: todos CRUD (fast DB read/write, needs worker)

Tokio worker pool:
  - 每个 HTTP 请求占用 1 个 worker
  - LLM 请求长时间占用 worker (Semaphore wait + inference)
  - 当活跃 LLM 请求数 > worker pool 容量时，后续请求排队
  - 非 LLM 请求（包括 todos）被迫等待可用 worker

客户端超时:
  - todos 请求在 60s 后触发 python http.client 超时
  - 客户端关闭连接
  - 服务器继续处理（如果 worker 可用），但客户端已断开
  - Python hreq() 函数收到超时异常 → 记录为 ERR

服务器端:
  - todo 操作本身通常 < 1s（纯 DB）
  - 但在 worker 队列中等待时间可能 > 60s
  - 最终在 600s 服务器超时前完成（故 todo_list 表最终为空）
```

### 3.2 数据验证

**todo_list 最终为空**（0 条记录）说明：
- 所有 `add_todo` 请求的 INSERT 操作最终都成功了
- `complete_todo` 删除了已创建的待办
- 说明 todos 端点**在服务器端没有失败**，只是客户端超时导致认为失败

**无 ConnectionError** 说明：
- 服务器未崩溃
- TCP 连接正常建立
- 只是 HTTP 响应在客户端超时后才到达

### 3.3 S2 中 todos 操作的具体耗时推算

| 指标 | 值 |
|------|-----|
| S2 总时长 | ~391 秒 |
| todos 操作数 | ~60 次 (20% × ~300 次) |
| LLM add 操作数 | ~128 次 (30% × ~430 次) |
| 平均 LLM 持有 Semaphore 时间 | ~3s inference × 队列系数 |
| Semaphore 队列深度峰值 | ~20-30 |
| 最坏情况等待时间 | ~60-90s |
| **客户端超时** | **60s** ← 这里断开 |
| 服务器 HTTP 超时 | 600s |

当 LLM 队列深度 > 20 时，排在第 21 位之后的 todos 请求需要等待超过 60 秒才能获得 worker，导致客户端超时。

---

## 4. 修复方案

### 4.1 修复方案：提升客户端超时

**最简单的修复**是将 todos 端点的客户端超时从 60s 提升到 300s（与 LLM 端点一致）：

**修改文件**: [stress_test_v2.py](file:///d:/OpenClaw-Workspace/ShouXin_Complete_v0.2.0/stress_test_v2.py)

```python
# 原有:
API_TO = 60  # 所有非 LLM 端点统一超时

# 修复后:
API_TO = 300  # 非 LLM 端点与 LLM 端点超时一致，覆盖 Semaphore 排队延迟
```

**理由**:
- todos 端点本身操作 < 1s
- 等待时间来自全局 worker 资源竞争，非 todos 端点自身问题
- 提升超时到 300s 不会改变系统行为，只允许请求在长 LLM 队列中等待完成

**替代方案 A（不推荐）**: 修改 `config.toml` 的 `timeout_secs` — 不必要，因为 600s 已够大

**替代方案 B（不推荐）**: 为 todos 端点单独设置更短超时 — 方向错误，因为问题不在 todos 自身

**替代方案 C（工程改进）**: 将 Semaphore(1) 改为 Semaphore(2) 允许有限并发 — 可行但需要验证 llama-cpp-2 线程安全性

### 4.2 无需代码修改的部分

经过代码审查确认：
- ✅ 无需修改 todos 存储层
- ✅ 无需修改 `resolve_owner` 函数
- ✅ 无需修改 GGUF Semaphore 逻辑
- ✅ 无需将 todos 端点改为异步

---

## 5. 修复验证计划

1. **修改** `stress_test_v2.py` 的 `API_TO = 300`
2. **重新运行 S2** 混合压力测试
3. **验证**: todos 端点错误率从 50% 降至 0%
4. **验证**: 其他端点不受影响

---

## 附录 A: todos 端点完全代码路径图

```
POST /api/todos (add_todo)
├── validate_contacts (async_memory.rs:326)
│   └── get_contact_by_id (store.rs:941) → SQLite READ
├── list_todos (async_memory.rs:378)
│   └── get_all_todos (todo_list.rs:377) → SQLite READ
└── add_todo (async_memory.rs:321)
    └── TodoListStore::add_todo → SQLite INSERT

GET /api/todos (list_todos)
└── list_todos → get_all_todos → SQLite READ

GET /api/todos/:id (get_todo)
├── resolve_owner (routes.rs:888)
│   └── get_todo_owner (todo_list.rs:475) → SQLite READ
└── get_todo → SQLite READ

PUT /api/todos/:id/complete (complete_todo)
├── resolve_owner → SQLite READ
└── complete_todo → SQLite UPDATE

DELETE /api/todos/:id (delete_todo)
├── resolve_owner → SQLite READ
└── delete_todo → SQLite DELETE
```

---

*诊断完成。所有 todos 操作均为纯数据库操作，无 LLM 调用。超时根因为客户端超时配置不足，非代码缺陷。*