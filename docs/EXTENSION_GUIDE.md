# ShouXin v0.5.1 扩展指南

## 一、添加新的 LLM 后端

ShouXin 支持多 LLM 后端（ollama、gguf、api）。

### 1. 实现 LlmClient Trait

在 `src/llm/` 下创建新文件，实现 `LlmClient` trait：

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn generate(&self, prompt: &str) -> Result<String, ShouxinError>;
    async fn preload(&self) -> Result<(), ShouxinError>;
    fn source_type(&self) -> LlmSourceType;
}
```

### 2. LlmOutputSanitizer 集成（v0.5.1）

如果自定义后端的输出格式与标准后端不同，建议在管道中启用 `LlmOutputSanitizer` 进行输出清洗：

```rust
use crate::memory_extractor::extractor::LlmOutputSanitizer;

let raw_output = client.generate(prompt).await?;
let cleaned = LlmOutputSanitizer::clean(&raw_output)?;
// cleaned 已过滤乱码、提取有效 JSON 片段
```

`LlmOutputSanitizer` 提供：
- **字符集检测**：验证输出是否为合法 UTF-8
- **乱码过滤**：去除不可打印字符和乱码片段
- **JSON 片段提取**：从混合输出中提取有效 JSON 块
- **格式修复**：修复截断 JSON、补全缺失括号

### 3. 注册到工厂

在 `src/llm/factory.rs` 的 `build_llm_manager()` 中添加新后端。

### 4. 更新配置

在 `src/config/settings.rs` 中添加对应的 BackendSettings 结构体。

## 二、添加新的存储后端

### 向量存储

实现 `VectorStore` trait：

```rust
#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn add(&self, id: &str, embedding: &[f32], metadata: &HashMap<String, String>) -> Result<(), ShouxinError>;
    async fn search(&self, query: &[f32], limit: usize) -> Result<Vec<(String, f32, HashMap<String, String>)>, ShouxinError>;
    async fn persist(&self) -> Result<usize, ShouxinError>;
}
```

在 `src/stores/vector/factory.rs` 注册新类型。

## 三、添加新的 API 路由

### 1. 定义请求/响应结构

在 `src/api/routes.rs` 中添加：

```rust
#[derive(Debug, Deserialize)]
pub struct MyNewRequest {
    pub field: String,
    pub scope: Option<Scope>,
    pub user_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MyNewResponse {
    pub result: String,
}
```

### 2. Scope 枚举（v0.5.1）

v0.5.1 中 Scope 枚举包含四种作用域：

```rust
pub enum Scope {
    User,       // 用户作用域
    Agent,      // Agent 作用域
    Contact,    // 联系人作用域
    Public,     // 公开作用域
}
```

所有 API 请求中可传入 `scope` 字段进行数据隔离，不同 Scope 下的数据互不可见。

### 3. 添加路由处理函数

```rust
async fn my_new_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MyNewRequest>,
) -> Result<Json<ApiResponse<MyNewResponse>>, AppError> {
    // 实现逻辑
    Ok(Json(ApiResponse::ok(MyNewResponse { result: "done".into() })))
}
```

### 4. 注册路由

在 `create_router()` 函数中添加：

```rust
.route("/api/my/new", routing::post(my_new_handler))
```

## 四、EventIndex 扩展字段

v0.5.1 中 EventIndex 新增了以下字段：

```rust
pub struct EventIndex {
    pub event_id: String,
    pub title: Option<String>,      // v0.5.0 新增
    pub event_type: String,
    pub summary: String,
    // v0.5.1 新增
    pub events_merged: u32,         // 已合并事件数
    pub contacts_merged: u32,       // 已合并联系人数
    pub ambiguities_detected: u32,  // 检测到的歧义数
    pub profiles_updated: u32,      // 已更新画像数
    // ... 其他字段
}
```

添加新字段时需同步更新：
1. `src/core/contact.rs` - EventIndex 结构体
2. `src/traits/store_traits.rs` - TraitEventIndex 和 EventDetail 结构体
3. `src/stores/relational/store.rs` - SQL 建表/SELECT/INSERT 语句
4. `src/memory/three_library_manager.rs` - 构造点
5. `src/service/background_queue.rs` - 构造点
6. `src/heartbeat/analyzer.rs` - 构造点
7. `src/service/internal_api.rs` - 构造点

## 五、TodoItem 扩展字段（v0.5.1）

v0.5.1 中 TodoItem 新增了归档相关字段：

```rust
pub struct TodoItem {
    pub todo_id: String,
    pub title: String,
    pub description: String,
    pub deadline: Option<DateTime<Utc>>,
    pub importance: Importance,
    pub status: TodoStatus,
    pub contact_ids: Vec<String>,
    pub source_event_id: Option<String>,
    pub owner_id: String,
    pub owner_type: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    // v0.5.1 新增
    pub archived_at: Option<DateTime<Utc>>,  // 归档时间
    pub todo_type: String,                   // 待办类型（normal / reminder）
    pub is_archived: bool,                   // 是否已归档
    pub last_updated_at: DateTime<Utc>,      // 最后更新时间
}
```

### TodoItem 唯一索引

P2-1 修复后，待办去重使用部分唯一索引：

```sql
CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_pending_todo
ON todo_list(title, owner_id, owner_type, deadline)
WHERE status = 'pending';
```

修改去重逻辑时需注意：
- 索引包含 `deadline` 字段
- 仅对 `status = 'pending'` 的记录生效
- 使用 `INSERT OR IGNORE` 配合 `rows_affected()` 检测冲突

### 归档机制（P2-2）

写入路径自动归档：当 pending 待办数量超过 50 时，`archive_stale_todos` 自动将创建超过 30 天的 pending 待办标记为归档。

### 提醒生成（P2-3）

写入路径自动提醒：当 pending 待办数量超过 30 时，`insert_reminder_todo` 自动生成提醒类型待办。

## 六、编译特性

```toml
[features]
default = []
gguf = ["llama-cpp-2"]      # 启用 GGUF 模型推理
gguf-cuda = ["llama-cpp-2"]  # 启用 GGUF CUDA 推理
```

添加新特性：在 `Cargo.toml` 的 `[features]` 中添加，并在代码中使用 `#[cfg(feature = "...")]` 条件编译。

## 七、配置系统

配置通过 `config.toml` 加载，支持环境变量覆盖：

```bash
# 环境变量格式: SHOUXIN_<SECTION>__<KEY>
SHOUXIN_LLM__PRIMARY=gguf
SHOUXIN_STORE_TYPE=hnsw
SHOUXIN_LLM__GGUF__MODEL_PATH=/path/to/model.gguf
```

添加新配置项：
1. 在 `src/config/settings.rs` 的对应 Settings 结构体中添加字段
2. 实现 `Default`
3. 在 `config.toml` 中添加默认值

## 八、后台提取队列

```rust
pub struct BackgroundTaskQueue {
    // 使用 with_stores() 创建
}

impl BackgroundTaskQueue {
    pub fn push(&self, task: ExtractionTask) -> Result<(), ShouxinError>;
    pub fn push_with_persist(&self, task: ExtractionTask) -> Result<(), ShouxinError>;
    pub fn recover_pending_tasks(&self);
}
```

提取任务通过 `DataDispatcher` 分发到三库存储。

## 九、文件上传异步化（P3-1）

文件上传采用混合模式：
- **同步阶段**：DB 插入元数据，立即返回 `FileRecord`
- **异步阶段**：`tokio::spawn` 执行文本提取（OCR/解析），不阻塞 API 响应

扩展新文件类型时，在 `src/service/file_processor.rs` 中注册对应的解析器即可。

## 十、代码规范

- 使用 `Arc<dyn Trait>` 进行依赖注入
- 所有异步操作使用 `async_trait`
- 错误处理使用 `ShouxinError` 枚举
- 数据隔离通过 `Scope` + `user_id` 实现
- 新增结构体字段时提供 `Default` 实现
- LLM 输出应在解析前经过 `LlmOutputSanitizer` 清洗