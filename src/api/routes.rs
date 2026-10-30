//! HTTP API 路由
//!
//! 使用 axum 提供 HTTP API
//!
//! # 端点列表
//! ## 记忆管理
//! - POST /api/memory/add - 添加对话
//! - POST /api/memory/search/contacts - 搜索联系人
//! - POST /api/memory/search/events - 搜索事件
//! - POST /api/memory/search/history - 搜索历史对话
//! - POST /api/memory/search/semantic - 语义搜索
//!
//! ## 待办事项
//! - POST /api/todos - 添加待办事项
//! - GET /api/todos - 获取待办列表
//! - GET /api/todos/{id} - 获取待办详情
//! - PUT /api/todos/{id}/complete - 完成待办
//! - DELETE /api/todos/{id} - 删除待办

use std::sync::Arc;

use axum::{
    extract::Path,
    extract::Query,
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::IntoResponse,
    body::Bytes,
    Extension,
    Json,
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::api::{AsyncMemory, TodoFilter};
use crate::core::contact::Scope;
use crate::error::ShouxinError;
use crate::heartbeat::{HeartbeatScheduler, HeartbeatReport, HeartbeatStatus, Question};
use crate::memory::three_library_manager::AddResult;
use crate::service::file_processor::FileProcessor;
use crate::stores::dictionary::SledDictionaryStore;
use crate::stores::relational::{FileIndex, FileStore, TodoItem};
use crate::stores::vector::types::{LearningMaterial, VectorSearchResult};
use crate::traits::embedder::Embedder;
use crate::traits::store_traits::{ContactDetail, Conversation, EventDetail, EventFilter};
use crate::traits::vector_store::VectorStore;
use tracing::{debug, error, info, warn};

// ==================== 请求结构体 ====================

/// 添加对话请求
#[derive(Debug, Deserialize)]
pub struct AddConversationRequest {
    /// 对话内容
    pub conversation: String,
    /// 用户 ID（可选）
    pub user_id: Option<String>,
    /// 操作范围（可选，默认为 User）
    #[serde(default)]
    pub scope: Scope,
    /// 对话发生时间（可选，缺省为当前时间）
    ///
    /// 支持 `RFC3339`（`2026-10-30T09:30:00+08:00`）、`YYYY-MM-DD HH:MM:SS`、
    /// `YYYY-MM-DD` 三种写法。导入历史聊天记录时必须带上真实时间，
    /// 否则全部落进"今天"这一页，按天分页就失效了。
    #[serde(default, alias = "occurred_at")]
    pub timestamp: Option<String>,
}

/// 搜索联系人请求
#[derive(Debug, Deserialize)]
pub struct SearchContactsRequest {
    /// 查询文本
    pub query: String,
    /// 结果数量限制
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// 操作范围（可选，默认为 User）
    #[serde(default, rename = "scope")]
    pub scope_raw: Option<String>,
    /// 用户 ID（scope=User 时用于隔离）
    #[serde(default)]
    pub user_id: Option<String>,
    /// 是否包含已归档联系人（热冷分离）
    #[serde(default)]
    pub include_archived: bool,
    /// 是否把虚拟角色（`is_virtual = 1`）混进普通联系人结果
    ///
    /// 默认 false：虚拟角色有自己的接口 `/api/virtual_entities`，
    /// 两套列表互不干扰（此前是混在一起的）。
    #[serde(default)]
    pub include_virtual: bool,
}

/// 搜索事件请求
#[derive(Debug, Deserialize)]
pub struct SearchEventsRequest {
    /// 用户 ID
    ///
    /// `owner_id` 作为别名接受：事件检索按 (owner_type=User, owner_id) 落库，
    /// 调用方很容易顺手写成 `owner_id`。静默忽略会直接返回空列表，
    /// 这种"看起来成功但什么也没有"的接口最难排查，因此在入口处对齐。
    #[serde(default, alias = "owner_id")]
    pub user_id: Option<String>,
    /// 事件类型
    pub event_type: Option<String>,
    /// 开始时间（RFC3339 格式）
    pub start_time: Option<String>,
    /// 结束时间（RFC3339 格式）
    pub end_time: Option<String>,
    /// 重要性
    pub importance: Option<String>,
    /// 状态
    pub status: Option<String>,
    /// 结果数量限制
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// 操作范围（可选，默认为 User）
    #[serde(default)]
    pub scope: Scope,
    /// 是否包含已归档事件（热冷分离）
    #[serde(default)]
    pub include_archived: bool,
    /// 按归档月份过滤（如 "2026-06"）
    #[serde(default)]
    pub archived_month: Option<String>,
}

/// 获取待确认事件合并请求
#[derive(Debug, Deserialize)]
pub struct GetPendingMergesRequest {
    /// 所有者类型（可选）
    #[serde(default)]
    pub owner_type: Option<String>,
    /// 所有者 ID（可选）
    pub owner_id: Option<String>,
    /// 结果数量限制
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// 确认事件合并请求
#[derive(Debug, Deserialize)]
pub struct ConfirmMergeRequest {
    /// 待确认合并 ID
    pub merge_id: i64,
    /// 操作：merge（合并）或 create（创建新事件）
    pub action: String,
    /// 源事件摘要（当 action=create 时使用）
    pub source_event_summary: Option<String>,
}

/// 待确认合并项
#[derive(Debug, Serialize)]
pub struct PendingMergeItem {
    pub id: i64,
    pub source_event_id: String,
    pub target_event_id: Option<String>,
    pub source_event_summary: String,
    pub target_event_summary: Option<String>,
    pub similarity_score: f32,
    pub status: String,
    pub created_at: String,
}

/// 确认合并响应
#[derive(Debug, Serialize)]
pub struct ConfirmMergeResponse {
    pub success: bool,
    pub message: String,
    pub event_id: Option<String>,
}

/// 搜索历史对话请求
#[derive(Debug, Deserialize)]
pub struct SearchHistoryRequest {
    /// 开始时间（RFC3339 格式，可选）
    #[serde(default)]
    pub start_time: Option<String>,
    /// 结束时间（RFC3339 格式，可选）
    #[serde(default)]
    pub end_time: Option<String>,
    /// 操作范围（可选，默认为 User）
    #[serde(default)]
    pub scope: Scope,
    /// 用户 ID（scope=User 时用于隔离）
    #[serde(default)]
    pub user_id: Option<String>,
}

/// 语义搜索请求
#[derive(Debug, Deserialize)]
pub struct SemanticSearchRequest {
    /// 查询文本
    pub query: String,
    /// 结果数量限制
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// 操作范围（可选，默认为 User）
    #[serde(default)]
    pub scope: Scope,
    /// 用户 ID（scope=User 时用于隔离）
    #[serde(default)]
    pub user_id: Option<String>,
    /// 源类型过滤（可选）
    #[serde(default)]
    pub source_types: Option<Vec<String>>,
    /// 对话 ID 过滤（可选，v0.5.1 新增）
    #[serde(default)]
    pub conversation_id: Option<String>,
}

/// 待办查询参数（用于 GET/PUT/DELETE 的 scope 和 user_id）
#[derive(Debug, Deserialize)]
pub struct ScopeQueryParams {
    #[serde(default)]
    pub scope: Option<Scope>,
    #[serde(default)]
    pub user_id: Option<String>,
}

/// 添加待办事项请求
#[derive(Debug, Deserialize)]
pub struct AddTodoRequest {
    /// 待办唯一 ID（可选，服务端会生成）
    pub todo_id: Option<String>,
    /// 关联联系人 ID 列表（可选）
    #[serde(default)]
    pub contact_ids: Option<Vec<String>>,
    /// 待办标题
    pub title: String,
    /// 详细描述
    pub description: Option<String>,
    /// 截止时间（RFC3339 格式，可选）
    ///
    /// 不填时默认"7 天后 23:59:59"，与抽取链路的兜底规则保持一致。
    #[serde(default)]
    pub deadline: Option<String>,
    /// 重要性（high / medium / low）
    #[serde(default = "default_importance")]
    pub importance: String,
    /// 关联的原始事件 ID
    pub source_event_id: Option<String>,
    /// 操作范围（可选，默认为 User）
    #[serde(default)]
    pub scope: Scope,
    /// 用户 ID（scope=User 时用于隔离）
    #[serde(default)]
    pub user_id: Option<String>,
}

/// I4: 验证 importance 值合法性
const VALID_IMPORTANCE: &[&str] = &["high", "medium", "low", "short_term", "long_term", "critical"];

fn validate_importance(value: &str) -> bool {
    VALID_IMPORTANCE.contains(&value)
}

/// I5: 待办标题最大长度
const MAX_TODO_TITLE_LEN: usize = 255;

/// 获取待办列表请求
#[derive(Debug, Deserialize)]
pub struct ListTodosRequest {
    /// 状态过滤
    pub status: Option<String>,
    /// 联系人 ID 过滤
    pub contact_id: Option<String>,
    /// 结果数量限制
    #[serde(default = "default_todo_limit")]
    pub limit: i64,
    /// 操作范围（可选，默认为 User）
    #[serde(default)]
    pub scope: Scope,
    /// 用户 ID（scope=User 时用于隔离）
    #[serde(default)]
    pub user_id: Option<String>,
    /// 是否包含已归档的待办
    #[serde(default)]
    pub include_archived: bool,
}

fn default_limit() -> usize {
    10
}
fn default_todo_limit() -> i64 {
    100
}
fn default_importance() -> String {
    "medium".to_string()
}

// ==================== 输入验证常量与辅助函数（S-09, S-10）====================

/// S-09: 对话内容最大字符数
const MAX_CONVERSATION_LEN: usize = 100_000;
/// S-09: 搜索查询最大字符数
const MAX_QUERY_LEN: usize = 1_000;
/// S-10: 搜索结果数量上限
const MAX_SEARCH_LIMIT: usize = 200;
/// S-09: 用户 ID 最大字符数
const MAX_USER_ID_LEN: usize = 128;

/// S-09: 验证 user_id 字符串格式（仅允许字母数字和下划线，最大 128 字符）
fn validate_user_id_str(uid: &str) -> Result<(), ShouxinError> {
    if uid.is_empty() {
        return Err(ShouxinError::InvalidArgument("user_id 不能为空".to_string()));
    }
    if uid.chars().count() > MAX_USER_ID_LEN {
        return Err(ShouxinError::InvalidArgument(format!(
            "user_id 长度超过限制（最大 {} 字符）",
            MAX_USER_ID_LEN
        )));
    }
    if !uid.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(ShouxinError::InvalidArgument(
            "user_id 仅允许字母数字和下划线".to_string(),
        ));
    }
    Ok(())
}

/// S-09: 验证 user_id（Option 形式）
fn validate_user_id(user_id: &Option<String>) -> Result<(), ShouxinError> {
    if let Some(uid) = user_id {
        validate_user_id_str(uid)?;
    }
    Ok(())
}

/// S-09: 验证搜索查询长度
fn validate_query_length(query: &str) -> Result<(), ShouxinError> {
    if query.chars().count() > MAX_QUERY_LEN {
        return Err(ShouxinError::InvalidArgument(format!(
            "查询内容过长（最大 {} 字符）",
            MAX_QUERY_LEN
        )));
    }
    Ok(())
}

/// S-10: 限制搜索结果数量上限
fn cap_limit(limit: usize) -> usize {
    std::cmp::min(limit, MAX_SEARCH_LIMIT)
}

// ==================== 响应结构体 ====================

/// API 响应
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    /// 是否成功
    pub success: bool,
    /// 响应数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// HTTP 状态码（不序列化到 JSON）
    #[serde(skip)]
    pub status_code: StatusCode,
}

impl<T> ApiResponse<T> {
    /// 创建成功响应
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            status_code: StatusCode::OK,
        }
    }

    /// 创建错误响应（默认 200 OK）
    pub fn error(error: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error),
            status_code: StatusCode::OK,
        }
    }

    /// 创建带自定义 HTTP 状态码的错误响应
    pub fn error_with_status(error: String, status: StatusCode) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error),
            status_code: status,
        }
    }
}

/// 实现 IntoResponse 以确保正确的 UTF-8 编码
impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> axum::response::Response {
        let body = serde_json::to_string(&self).unwrap_or_else(|_| {
            r#"{"success":false,"error":"Internal serialization error"}"#.to_string()
        });
        axum::response::Response::builder()
            .status(self.status_code)
            .header(axum::http::header::CONTENT_TYPE, "application/json; charset=utf-8")
            .body(axum::body::Body::from(body))
            .unwrap_or_else(|_| {
                axum::response::Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(axum::body::Body::from("Internal error"))
                    .unwrap()
            })
    }
}

/// 添加对话响应
#[derive(Debug, Serialize)]
pub struct AddConversationResponse {
    /// 新增的联系人列表
    pub contacts: Vec<ContactDetail>,
    /// 新增的事件列表
    pub events: Vec<EventDetail>,
    /// 新增的学习素材列表
    pub learnings: Vec<LearningMaterial>,
    /// 对话 ID
    pub conversation_id: Option<String>,
}

impl From<AddResult> for AddConversationResponse {
    fn from(result: AddResult) -> Self {
        Self {
            contacts: result.contacts,
            events: result.events,
            learnings: result.learnings,
            conversation_id: result.conversation_id,
        }
    }
}

// ==================== 文件管理 API 状态 ====================

pub struct FileApiState {
    pub file_store: FileStore,
    pub dictionary: Arc<SledDictionaryStore>,
    pub vector_store: Arc<dyn VectorStore>,
    pub embedder: Arc<dyn Embedder>,
    pub data_dir: String,
}

impl FileApiState {
    pub fn processor(&self) -> FileProcessor {
        FileProcessor::new(
            self.file_store.clone(),
            self.dictionary.clone(),
            self.vector_store.clone(),
            self.embedder.clone(),
        )
    }
}

#[derive(Debug, Deserialize)]
struct UploadFileQuery {
    filename: String,
    #[serde(default = "default_scope")]
    scope: String,
    #[serde(default = "default_user_id")]
    user_id: String,
    #[serde(default)]
    target_type: Option<String>,
    #[serde(default)]
    target_id: Option<String>,
}

fn default_scope() -> String { "agent".to_string() }
/// 请求未带 owner 时的默认 owner_id
///
/// 默认作用域是 `agent`，因此这里必须与 `settings.agent.primary_agent_id` 一致，
/// 而不是写死的历史值（F-T1）。
fn default_user_id() -> String { crate::core::owner::primary_agent_id().to_string() }

/// 文件读/删接口的归属查询参数
///
/// `get_file_metadata` / `get_file_text` / `delete_file` 必须以 owner 为准做校验，
/// 否则任意 file_id 都能跨 owner 读取或删除（F-F3）。默认值与上传接口保持一致。
#[derive(Debug, Deserialize)]
struct FileOwnerQuery {
    #[serde(default = "default_scope")]
    scope: String,
    #[serde(default = "default_user_id")]
    user_id: String,
}

/// 判断文件是否属于请求方
fn file_owned_by(file: &FileIndex, owner: &FileOwnerQuery) -> bool {
    file.owner_type.eq_ignore_ascii_case(&owner.scope) && file.owner_id == owner.user_id
}

/// 根据文件名扩展名检测 MIME 类型（P0-2: 修复 mime_type 为 NULL 的问题）
fn detect_mime_type(filename: &str) -> String {
    let lower = filename.to_lowercase();
    if lower.ends_with(".txt") {
        "text/plain".to_string()
    } else if lower.ends_with(".md") || lower.ends_with(".markdown") {
        "text/markdown".to_string()
    } else if lower.ends_with(".pdf") {
        "application/pdf".to_string()
    } else if lower.ends_with(".docx") {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string()
    } else if lower.ends_with(".xlsx") {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string()
    } else if lower.ends_with(".csv") {
        "text/csv".to_string()
    } else if lower.ends_with(".py") {
        "text/x-python".to_string()
    } else if lower.ends_with(".js") {
        "application/javascript".to_string()
    } else if lower.ends_with(".ts") {
        "application/typescript".to_string()
    } else if lower.ends_with(".rs") {
        "text/x-rust".to_string()
    } else if lower.ends_with(".go") {
        "text/x-go".to_string()
    } else if lower.ends_with(".java") {
        "text/x-java".to_string()
    } else if lower.ends_with(".c") || lower.ends_with(".cpp") || lower.ends_with(".h") {
        "text/x-c".to_string()
    } else if lower.ends_with(".html") {
        "text/html".to_string()
    } else if lower.ends_with(".css") {
        "text/css".to_string()
    } else if lower.ends_with(".json") {
        "application/json".to_string()
    } else if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        "application/yaml".to_string()
    } else if lower.ends_with(".toml") {
        "application/toml".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

/// 将内部状态映射为 API 规范定义的 processing_status
/// 内部状态: pending / completed / failed
/// 规范状态: metadata_ready / extracting / ready / failed
fn map_processing_status(internal_status: &str) -> &'static str {
    match internal_status {
        "pending" => "metadata_ready",
        "completed" => "ready",
        "failed" => "failed",
        _ => "extracting",
    }
}

// ==================== 文件管理端点处理器 ====================

async fn upload_file(
    Extension(state): Extension<Arc<FileApiState>>,
    Query(query): Query<UploadFileQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // P0-2: 严格按照 API 规范验证 Content-Type（必须为 application/octet-stream）
    // 禁止 multipart/form-data，避免文件内容被 boundary 头污染导致 MD5 去重失效
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.starts_with("application/octet-stream") {
        return ApiResponse::error_with_status(
            format!(
                "Content-Type 必须为 application/octet-stream（当前: {}）。文件上传不支持 multipart/form-data，请使用原始字节流，文件名通过 query 参数 filename 传递。",
                if content_type.is_empty() { "未指定" } else { content_type }
            ),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        );
    }

    if body.is_empty() {
        return ApiResponse::error_with_status(
            "未收到文件内容".to_string(),
            StatusCode::BAD_REQUEST,
        );
    }

    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id_str(&query.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }

    const MAX_FILE_SIZE: usize = 20 * 1024 * 1024;
    if body.len() > MAX_FILE_SIZE {
        return ApiResponse::error_with_status(
            format!(
                "文件大小超过限制（最大 {}MB）",
                MAX_FILE_SIZE / 1024 / 1024
            ),
            StatusCode::PAYLOAD_TOO_LARGE,
        );
    }

    let file_name = query.filename;
    let file_type = FileProcessor::detect_file_type(&file_name);
    let mime_type = detect_mime_type(&file_name);

    const ALLOWED_TYPES: &[&str] = &["txt", "md", "code", "pdf", "doc", "xls", "csv"];
    if !ALLOWED_TYPES.contains(&file_type.as_str()) {
        return ApiResponse::error_with_status(
            format!(
                "不支持的文件类型: {}。支持的类型: {}",
                file_type,
                ALLOWED_TYPES.join(", ")
            ),
            StatusCode::BAD_REQUEST,
        );
    }

    let scope = query.scope;
    let user_id = query.user_id;

    let file_size = body.len() as i64;
    let md5_hash = format!("{:x}", md5::compute(&body));

    // P0-2: MD5 去重检查 — 返回符合 API 规范的完整响应
    if let Ok(Some(existing)) = state.file_store.get_file_by_md5_and_owner(&md5_hash, &scope, &user_id).await {
        let processing_status = map_processing_status(&existing.status);
        return ApiResponse::success(serde_json::json!({
            "file_id": existing.file_id,
            "filename": existing.file_name,
            "md5": existing.md5_hash,
            "size": existing.file_size,
            "file_type": existing.file_type,
            "processing_status": processing_status,
            "is_duplicate": true,
            "uploaded_at": existing.uploaded_at
        }));
    }

    let file_id = uuid::Uuid::new_v4().to_string();

    // 同步写入数据库记录（快速），确保元数据立即可查
    let now = Utc::now().to_rfc3339();
    let temp_dir = std::path::Path::new(&state.data_dir).join("temp_uploads");
    std::fs::create_dir_all(&temp_dir).ok();
    let temp_path = temp_dir.join(&file_id);
    if let Err(e) = tokio::fs::write(&temp_path, &body).await {
        error!("保存临时文件失败: {}", e);
        return ApiResponse::error_with_status("内部错误".to_string(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    let file_index = FileIndex {
        file_id: file_id.clone(),
        file_name: file_name.clone(),
        file_type: file_type.clone(),
        mime_type: Some(mime_type),
        file_size: Some(file_size),
        md5_hash: Some(md5_hash.clone()),
        owner_type: scope.clone(),
        owner_id: user_id.clone(),
        uploaded_at: Some(now.clone()),
        processed_at: None,
        status: "pending".to_string(),
        error_message: None,
        original_path: Some(temp_path.to_string_lossy().to_string()),
        extracted_text_summary: None,
    };

    match state.file_store.insert_file(&file_index).await {
        Ok(true) => {}
        Ok(false) => {
            // F-F1: 并发去重竞态——同 md5+owner 已被另一请求插入。
            // 清理本次写入的临时文件，返回既有记录，而不是返回一个不存在的 file_id。
            let _ = tokio::fs::remove_file(&temp_path).await;
            match state.file_store.get_file_by_md5_and_owner(&md5_hash, &scope, &user_id).await {
                Ok(Some(existing)) => {
                    return ApiResponse::success(serde_json::json!({
                        "file_id": existing.file_id,
                        "filename": existing.file_name,
                        "md5": existing.md5_hash,
                        "size": existing.file_size,
                        "file_type": existing.file_type,
                        "processing_status": map_processing_status(&existing.status),
                        "is_duplicate": true,
                        "uploaded_at": existing.uploaded_at
                    }));
                }
                _ => {
                    return ApiResponse::error_with_status(
                        "文件已存在".to_string(),
                        StatusCode::CONFLICT,
                    );
                }
            }
        }
        Err(e) => {
            error!("文件记录保存失败: {}", e);
            let _ = tokio::fs::remove_file(&temp_path).await;
            return ApiResponse::error_with_status("内部错误".to_string(), StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    if let (Some(ref tt), Some(ref tid)) = (&query.target_type, &query.target_id) {
        if let Err(e) = state.file_store.insert_file_relation(&file_id, tt, tid).await {
            eprintln!("[FileAPI] 插入文件关联失败: {}", e);
        }
    }

    // 异步化：仅将耗时的文件处理（文本提取、索引）移到后台任务
    let processor = state.processor();
    let fid = file_id.clone();
    let fname = file_name.clone();

    tokio::spawn(async move {
        println!("[FileAPI] Spawning background processing for file: {} ({})", fid, fname);
        match processor.process_file(&fid).await {
            Ok(()) => println!("[FileAPI] File {} processed successfully", fid),
            Err(e) => {
                eprintln!("[FileAPI] File {} processing FAILED: {}", fid, e);
                tracing::error!("文件 {} 处理失败: {}", fid, e);
            }
        }
    });

    // P0-2: 返回符合 API 规范的响应结构
    ApiResponse::success(serde_json::json!({
        "file_id": file_id,
        "filename": file_name,
        "md5": md5_hash,
        "size": file_size,
        "file_type": file_type,
        "processing_status": "metadata_ready",
        "is_duplicate": false,
        "uploaded_at": now
    }))
}

async fn get_file_metadata(
    Extension(state): Extension<Arc<FileApiState>>,
    Path(file_id): Path<String>,
    Query(owner): Query<FileOwnerQuery>,
) -> impl IntoResponse {
    match state.file_store.get_file(&file_id).await {
        // 归属不匹配时统一返回"文件不存在"，避免暴露其他 owner 的文件是否存在
        Ok(Some(file)) if !file_owned_by(&file, &owner) => {
            ApiResponse::error("文件不存在".to_string())
        }
        Ok(Some(file)) => ApiResponse::success(serde_json::to_value(file).unwrap_or_default()),
        Ok(None) => ApiResponse::error("文件不存在".to_string()),
        Err(e) => {
            error!("查询文件元数据失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

async fn get_file_text(
    Extension(state): Extension<Arc<FileApiState>>,
    Path(file_id): Path<String>,
    Query(owner): Query<FileOwnerQuery>,
) -> impl IntoResponse {
    // 先校验归属，再读全文
    match state.file_store.get_file(&file_id).await {
        Ok(Some(file)) if file_owned_by(&file, &owner) => {}
        Ok(_) => return ApiResponse::error("文件不存在".to_string()),
        Err(e) => {
            error!("查询文件归属失败: {}", e);
            return ApiResponse::error("内部错误".to_string());
        }
    }

    match state.dictionary.get_file_text(&file_id) {
        Ok(Some(data)) => {
            match serde_json::from_slice::<serde_json::Value>(&data) {
                Ok(doc) => {
                    let full_text = doc.get("full_text").and_then(|v| v.as_str()).unwrap_or("");
                    ApiResponse::success(serde_json::json!({"file_id": file_id, "full_text": full_text}))
                }
                Err(_) => ApiResponse::error("文本解码失败".to_string()),
            }
        }
        Ok(None) => ApiResponse::error("文件文本不存在或尚未处理完成".to_string()),
        Err(e) => {
            error!("查询文件文本失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

async fn delete_file(
    Extension(state): Extension<Arc<FileApiState>>,
    Path(file_id): Path<String>,
    Query(owner): Query<FileOwnerQuery>,
) -> impl IntoResponse {
    // 归属校验：不允许跨 owner 删除
    let file = match state.file_store.get_file(&file_id).await {
        Ok(Some(file)) if file_owned_by(&file, &owner) => file,
        Ok(_) => return ApiResponse::error("文件不存在".to_string()),
        Err(e) => {
            error!("查询文件归属失败: {}", e);
            return ApiResponse::error("内部错误".to_string());
        }
    };

    if let Err(e) = state.file_store.delete_file(&file_id).await {
        error!("删除文件记录失败: {}", e);
        return ApiResponse::error("内部错误".to_string());
    }

    let _ = state.dictionary.delete_file_text(&file_id);

    // F-F2: pending/failed 的临时文件在成功处理路径才会被删除，
    // 这里补上删除，避免留下无主文件。
    if let Some(ref original_path) = file.original_path {
        if file.status != "completed" {
            if let Err(e) = tokio::fs::remove_file(original_path).await {
                warn!("删除临时文件失败 {}: {}", original_path, e);
            }
        }
    }

    ApiResponse::success(serde_json::json!({"file_id": file_id, "deleted": true}))
}

// ==================== 路由创建 ====================

/// 创建路由
///
/// # 参数
/// - `async_memory`: 异步记忆管理实例
///
/// # 返回
/// axum Router 实例
pub fn create_router(async_memory: Arc<AsyncMemory>, file_state: Arc<FileApiState>, heartbeat_scheduler: Arc<HeartbeatScheduler>) -> Router {
    // V4: 文件上传路由独立，禁用 body limit（handler 内部有 MAX_FILE_SIZE 检查）
    let file_routes = Router::new()
        .route("/api/files/upload", axum::routing::post(upload_file))
        .route("/api/files/:file_id", axum::routing::get(get_file_metadata))
        .route("/api/files/:file_id/text", axum::routing::get(get_file_text))
        .route("/api/files/:file_id", axum::routing::delete(delete_file))
        .layer(Extension(file_state.clone()))
        .layer(DefaultBodyLimit::disable());

    Router::new()
        // 健康检查
        .route("/health", axum::routing::get(health_check))
        // 记忆管理端点
        .route("/api/memory/add", axum::routing::post(add_conversation))
        .route("/api/memory/extract_status/:conversation_id", axum::routing::get(extract_status))
        .route(
            "/api/memory/replay/:conversation_id",
            axum::routing::post(replay_conversation),
        )
        .route(
            "/api/memory/replay_failed",
            axum::routing::post(replay_failed_conversations),
        )
        .route(
            "/api/events/pending_merges",
            axum::routing::get(list_pending_event_merges),
        )
        .route(
            "/api/events/pending_merges/:merge_id/resolve",
            axum::routing::post(resolve_pending_event_merge),
        )
        .route(
            "/api/memory/search/contacts",
            axum::routing::post(search_contacts),
        )
        .route(
            "/api/contacts/:id/detail",
            axum::routing::get(get_contact_detail),
        )
        .route(
            "/api/contacts/:id/event_archive",
            axum::routing::get(get_contact_event_archive),
        )
        // 字典库（原文）按天分页
        .route(
            "/api/dictionary/days",
            axum::routing::get(list_dictionary_days),
        )
        .route(
            "/api/dictionary/day/:date",
            axum::routing::get(get_dictionary_day),
        )
        .route(
            "/api/dictionary/conversation/:id",
            axum::routing::get(get_dictionary_conversation),
        )
        .route(
            "/api/dictionary/stats",
            axum::routing::get(dictionary_stats),
        )
        .route(
            "/api/dictionary/paging",
            axum::routing::get(dictionary_paging_info),
        )
        .route(
            "/api/memory/search/events",
            axum::routing::post(search_events),
        )
        .route(
            "/api/memory/search/history",
            axum::routing::post(search_history),
        )
        // 事件合并管理端点
        .route(
            "/api/events/pending_merges",
            axum::routing::post(get_pending_event_merges),
        )
        .route(
            "/api/events/confirm_merge",
            axum::routing::post(confirm_event_merge),
        )
        .route(
            "/api/memory/search/semantic",
            axum::routing::post(semantic_search),
        )
        // 向量检索端点
        .route("/api/vector/search", axum::routing::post(semantic_search))
        .route("/api/vector/:id", axum::routing::get(get_learning_material))
        // 待办事项端点
        .route("/api/todos", axum::routing::post(add_todo))
        .route("/api/todos", axum::routing::get(list_todos))
        .route("/api/todos/:id", axum::routing::get(get_todo))
        .route("/api/todos/:id/complete", axum::routing::put(complete_todo))
        .route("/api/todos/:id", axum::routing::delete(delete_todo))
        // 心跳模块端点
        .route("/api/heartbeat/trigger", axum::routing::post(trigger_heartbeat))
        .route("/api/heartbeat/pending_questions", axum::routing::get(get_pending_questions))
        .route("/api/heartbeat/status", axum::routing::get(get_heartbeat_status))
        .route("/api/heartbeat/answer", axum::routing::post(answer_question))
        // 智能搜索端点
        .route("/api/search/smart", axum::routing::post(smart_search))
        // 虚拟角色管理端点
        .route("/api/virtual_entities", axum::routing::get(list_virtual_entities))
        .route("/api/virtual_entities/:id", axum::routing::get(get_virtual_entity))
        .route("/api/virtual_entities", axum::routing::post(add_virtual_entity))
        .route("/api/virtual_entities/:id", axum::routing::put(update_virtual_entity))
        .route("/api/virtual_entities/:id", axum::routing::delete(delete_virtual_entity))
        .route("/api/virtual_entities/check/:name", axum::routing::get(check_virtual_entity_match))
        .merge(file_routes)
        .layer(Extension(async_memory))
        .layer(Extension(file_state))
        .layer(Extension(heartbeat_scheduler))
}

// ==================== 健康检查 ====================

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "time": Utc::now().to_rfc3339()}))
}

// ==================== 字典库分页查询 ====================

/// 字典库查询参数（按天分页）
#[derive(Debug, Deserialize)]
pub struct DictionaryQueryParams {
    #[serde(default)]
    pub scope: Option<Scope>,
    #[serde(default)]
    pub user_id: Option<String>,
    /// 单页条数（默认 50，上限 200）
    #[serde(default = "default_dictionary_limit")]
    pub limit: usize,
    /// 页内偏移（默认 0）
    #[serde(default)]
    pub offset: usize,
    /// 只列出该日期之前（不含）的会话日，用于翻页
    #[serde(default)]
    pub before: Option<String>,
}

fn default_dictionary_limit() -> usize {
    50
}

/// 把查询参数解析成 (owner_type, owner_id)
///
/// 字典库是"原文"，跨 owner 泄漏比结构化数据更严重，因此默认按 User + 默认用户过滤。
fn dictionary_owner(params: &DictionaryQueryParams) -> (crate::core::contact::OwnerType, String) {
    params
        .scope
        .unwrap_or(Scope::User)
        .to_owner_params(params.user_id.as_deref())
}

/// 字典库分页口径（截止小时 + 时区偏移），供上层与本地展示对齐
async fn dictionary_paging_info(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
) -> ApiResponse<serde_json::Value> {
    let dict = async_memory.get_dictionary_store();
    match dict.dictionary_stats().await {
        Ok(stats) => ApiResponse::success(serde_json::json!({
            "cutoff_hour": stats.cutoff_hour,
            "utc_offset_minutes": stats.utc_offset_minutes,
            "rule": "会话日：本地时间 cutoff_hour 之前算前一天（凌晨兜底，避免零点把一次会话切成两页）",
        })),
        Err(e) => {
            error!("读取字典库分页配置失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 列出有对话的会话日（倒序），用于"按天翻页"
async fn list_dictionary_days(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Query(params): Query<DictionaryQueryParams>,
) -> ApiResponse<serde_json::Value> {
    if let Err(e) = validate_user_id(&params.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    let dict = async_memory.get_dictionary_store();
    let (owner_type, owner_id) = dictionary_owner(&params);
    let limit = params.limit.clamp(1, 366);
    let before = params
        .before
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match dict
        .list_day_pages(before, Some(owner_type), Some(&owner_id), limit)
        .await
    {
        Ok(days) => {
            let next_before = days.last().map(|d| d.day.clone());
            let full = days.len() == limit;
            ApiResponse::success(serde_json::json!({
                "owner_type": owner_type.as_str(),
                "owner_id": owner_id,
                "count": days.len(),
                "next_before": if full { next_before } else { None },
                "days": days,
            }))
        }
        Err(e) => {
            error!("列出字典库会话日失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 读取某个会话日的对话（按时间升序，分页）
async fn get_dictionary_day(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(day): Path<String>,
    Query(params): Query<DictionaryQueryParams>,
) -> ApiResponse<serde_json::Value> {
    if let Err(e) = validate_user_id(&params.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    if chrono::NaiveDate::parse_from_str(day.trim(), "%Y-%m-%d").is_err() {
        return ApiResponse::error_with_status(
            format!("日期格式应为 YYYY-MM-DD：{}", day),
            StatusCode::BAD_REQUEST,
        );
    }
    let dict = async_memory.get_dictionary_store();
    let (owner_type, owner_id) = dictionary_owner(&params);
    let limit = params.limit.clamp(1, 200);
    let offset = params.offset.min(100_000);

    match dict
        .get_day_page(&day, Some(owner_type), Some(&owner_id), limit, offset)
        .await
    {
        Ok(records) => {
            let items: Vec<serde_json::Value> = records
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "conversation_id": r.id,
                        "timestamp": r.timestamp.to_rfc3339(),
                        "session_day": if r.session_day.is_empty() { day.clone() } else { r.session_day.clone() },
                        "ingested_at": r.ingested_at.map(|t| t.to_rfc3339()),
                        "owner_type": r.owner_type.as_str(),
                        "owner_id": r.owner_id,
                        "extracted_status": r.extracted_status,
                        "extraction_error": r.extraction_error,
                        "chars": r.content.chars().count(),
                        "preview": r.content.chars().take(120).collect::<String>(),
                    })
                })
                .collect();
            let has_more = items.len() == limit;
            ApiResponse::success(serde_json::json!({
                "day": day,
                "owner_type": owner_type.as_str(),
                "owner_id": owner_id,
                "offset": offset,
                "limit": limit,
                "count": items.len(),
                "has_more": has_more,
                "conversations": items,
            }))
        }
        Err(e) => {
            error!("读取字典库某天对话失败 {}: {}", day, e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 字典库自检统计
async fn dictionary_stats(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
) -> ApiResponse<serde_json::Value> {
    let dict = async_memory.get_dictionary_store();
    match dict.dictionary_stats().await {
        Ok(stats) => ApiResponse::success(serde_json::json!(stats)),
        Err(e) => {
            error!("字典库统计失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 按 ID 读取一条对话原文（用于回溯）
async fn get_dictionary_conversation(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(id): Path<String>,
    Query(params): Query<DictionaryQueryParams>,
) -> ApiResponse<serde_json::Value> {
    let dict = async_memory.get_dictionary_store();
    let (owner_type, owner_id) = dictionary_owner(&params);
    match dict.get_conversation_by_id(&id).await {
        Ok(Some(conv)) => {
            // 越权防护：只返回当前 owner 的原文
            let belongs = match owner_type {
                crate::core::contact::OwnerType::User => conv
                    .user_id
                    .as_deref()
                    .map(|u| u == owner_id)
                    .unwrap_or(false),
                crate::core::contact::OwnerType::Agent => conv
                    .agent_id
                    .as_deref()
                    .map(|a| a == owner_id)
                    .unwrap_or(false),
                _ => true,
            };
            if !belongs {
                return ApiResponse::error_with_status(
                    "conversation not found".to_string(),
                    StatusCode::NOT_FOUND,
                );
            }
            ApiResponse::success(serde_json::json!({
                "conversation_id": conv.id,
                "content": conv.content,
                "timestamp": conv.created_at.to_rfc3339(),
                "extracted_status": conv.extracted_status,
                "extraction_error": conv.extraction_error,
                "chars": conv.content.chars().count(),
            }))
        }
        Ok(None) => ApiResponse::error_with_status(
            format!("conversation not found: {}", id),
            StatusCode::NOT_FOUND,
        ),
        Err(e) => {
            error!("读取字典库对话失败 {}: {}", id, e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 联系人在字典库中的历史事件档案（永久副本，不随关系库归档/合并消失）
async fn get_contact_event_archive(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(contact_id): Path<String>,
    Query(params): Query<DictionaryQueryParams>,
) -> ApiResponse<serde_json::Value> {
    let dict = async_memory.get_dictionary_store();
    let limit = params.limit.clamp(1, 500);
    match dict.get_contact_events(&contact_id, Some(limit)).await {
        Ok(events) => {
            let items: Vec<serde_json::Value> = events
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "event_id": e.event_id,
                        "event_time": e.event_time.to_rfc3339(),
                        "summary": e.event_summary,
                        "importance": e.importance,
                        "user_sentiment": e.user_sentiment,
                        "conversation_id": e.raw_conversation_id,
                    })
                })
                .collect();
            ApiResponse::success(serde_json::json!({
                "contact_id": contact_id,
                "count": items.len(),
                "events": items,
            }))
        }
        Err(e) => {
            error!("读取联系人事件档案失败 {}: {}", contact_id, e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 解析对话发生时间
///
/// 支持 RFC3339、`YYYY-MM-DD HH:MM[:SS]`、`YYYY-MM-DD`。
/// 不带时区的写法按**本地时间**解释（用户说"9 点半"指的是自己所在时区）。
fn parse_conversation_timestamp(raw: &str) -> Result<chrono::DateTime<Utc>, String> {
    use chrono::TimeZone;

    let s = raw.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            if let Some(local) = chrono::Local.from_local_datetime(&naive).single() {
                return Ok(local.with_timezone(&Utc));
            }
            // 夏令时回拨导致本地时间歧义时按 UTC 解释，避免整体失败
            return Ok(naive.and_utc());
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        // 只给日期时取当天 12:00（本地）：远离零点与 cutoff，不会被分到相邻的页
        let naive = date
            .and_hms_opt(12, 0, 0)
            .ok_or_else(|| format!("非法日期：{}", raw))?;
        if let Some(local) = chrono::Local.from_local_datetime(&naive).single() {
            return Ok(local.with_timezone(&Utc));
        }
        return Ok(naive.and_utc());
    }
    Err(format!(
        "无法解析（支持 2026-10-30T09:30:00+08:00 / 2026-10-30 09:30:00 / 2026-10-30）：{}",
        raw
    ))
}

// ==================== 记忆管理处理器 ====================

/// 添加对话
async fn add_conversation(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<AddConversationRequest>,
) -> ApiResponse<AddConversationResponse> {
    // 空内容直接拒绝：否则会写入一条无意义对话并占用抽取队列
    let trimmed_len = req.conversation.trim().chars().count();
    if trimmed_len == 0 {
        return ApiResponse::error_with_status(
            "对话内容为空".to_string(),
            StatusCode::BAD_REQUEST,
        );
    }
    // S-09: 验证 conversation 长度
    if req.conversation.chars().count() > MAX_CONVERSATION_LEN {
        return ApiResponse::error_with_status(
            format!("对话内容过长（最大 {} 字符）", MAX_CONVERSATION_LEN),
            StatusCode::BAD_REQUEST,
        );
    }
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&req.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }

    // 对话发生时间：可缺省（= 现在）。给了就必须能解析且在合理区间内，
    // 否则历史数据会被错误地分到今天的页里，事后无法纠正。
    let occurred_at = match req
        .timestamp
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(raw) => match parse_conversation_timestamp(raw) {
            Ok(ts) => {
                let now = Utc::now();
                if ts > now + chrono::Duration::days(1) {
                    return ApiResponse::error_with_status(
                        format!("timestamp 超出合理范围（不能晚于当前时间）：{}", raw),
                        StatusCode::BAD_REQUEST,
                    );
                }
                if ts < chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or(now)
                {
                    return ApiResponse::error_with_status(
                        format!("timestamp 超出合理范围（早于 2000-01-01）：{}", raw),
                        StatusCode::BAD_REQUEST,
                    );
                }
                Some(ts)
            }
            Err(e) => {
                return ApiResponse::error_with_status(
                    format!("timestamp 解析失败：{}", e),
                    StatusCode::BAD_REQUEST,
                )
            }
        },
    };

    match async_memory
        .add_async_at(&req.conversation, req.user_id.as_deref(), req.scope, occurred_at)
        .await
    {
        Ok(result) => {
            let response = AddConversationResponse::from(result);
            ApiResponse::success(response)
        }
        Err(e) => {
            error!("添加对话失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 查询提取状态 (v0.5.1)
async fn extract_status(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(conversation_id): Path<String>,
) -> ApiResponse<serde_json::Value> {
    let dict_store = async_memory.get_dictionary_store();
    match dict_store.get_conversation_by_id(&conversation_id).await {
        Ok(Some(conv)) => {
            let status = serde_json::json!({
                "conversation_id": conversation_id,
                "status": conv.extracted_status,
                "extraction_error": conv.extraction_error,
                "created_at": conv.created_at.to_rfc3339(),
                "content_preview": conv.content.chars().take(200).collect::<String>(),
            });
            ApiResponse::success(status)
        }
        Ok(None) => {
            ApiResponse::error_with_status(
                format!("conversation not found: {}", conversation_id),
                StatusCode::NOT_FOUND,
            )
        }
        Err(e) => {
            error!("查询提取状态失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 重放某条对话的提取任务
///
/// 用于失败/中断后的恢复：把字典库里的原文重新入队，状态回到 `pending`。
/// 不涉及任何字典数据删除，重复重放由写入侧去重规则兜底。
async fn replay_conversation(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(conversation_id): Path<String>,
) -> ApiResponse<serde_json::Value> {
    match async_memory.replay_conversation(&conversation_id).await {
        Ok(()) => ApiResponse::success(serde_json::json!({
            "conversation_id": conversation_id,
            "replayed": true,
            "status": "pending"
        })),
        Err(e) => {
            error!("重放提取任务失败 {}: {}", conversation_id, e);
            ApiResponse::error_with_status(
                format!("replay failed: {}", e),
                StatusCode::BAD_REQUEST,
            )
        }
    }
}

/// 批量重放失败对话的参数
#[derive(Debug, Deserialize)]
struct ReplayFailedQuery {
    #[serde(default)]
    limit: Option<usize>,
}

/// 批量重放失败的对话提取任务
///
/// `POST /api/memory/replay_failed?limit=20`，limit 默认 20、上限 200。
async fn replay_failed_conversations(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Query(query): Query<ReplayFailedQuery>,
) -> ApiResponse<serde_json::Value> {
    let limit = query.limit.unwrap_or(20).clamp(1, 200);
    match async_memory.replay_failed_conversations(limit).await {
        Ok((succeeded, failed)) => ApiResponse::success(serde_json::json!({
            "limit": limit,
            "succeeded": succeeded,
            "failed": failed
        })),
        Err(e) => {
            error!("批量重放失败: {}", e);
            ApiResponse::error_with_status("内部错误".to_string(), StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// 待确认事件合并的查询参数
#[derive(Debug, Deserialize)]
struct PendingMergeQuery {
    /// 所有者类型：User / Agent
    owner_type: Option<String>,
    /// 所有者 ID
    owner_id: Option<String>,
    /// 返回条数上限
    limit: Option<usize>,
}

/// 列出"待确认合并"的事件对
///
/// 事件相似度落在 0.75–0.90 区间时不会自动合并（避免误合并），
/// 需要人工/外部确认；此前没有任何入口，这里补上查询与处置接口。
async fn list_pending_event_merges(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Query(query): Query<PendingMergeQuery>,
) -> ApiResponse<serde_json::Value> {
    let rel_store = async_memory.get_relational_store();
    let owner_type = query
        .owner_type
        .as_deref()
        .and_then(crate::core::contact::OwnerType::parse);

    match rel_store
        .get_pending_event_merges(owner_type, query.owner_id.as_deref(), query.limit)
        .await
    {
        Ok(merges) => {
            let count = merges.len();
            ApiResponse::success(serde_json::json!({
                "count": count,
                "merges": merges
            }))
        }
        Err(e) => {
            error!("查询待确认合并失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 处置一条"待确认合并"
#[derive(Debug, Deserialize)]
struct ResolveMergeRequest {
    /// merge=合并到目标事件 / create=保留为独立事件 / dismiss=忽略
    action: String,
}

/// 确认或拒绝一条待合并事件
///
/// action 取值：`merge`（合并）、`create`（保留为独立事件）、`dismiss`（忽略）。
async fn resolve_pending_event_merge(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(merge_id): Path<i64>,
    Json(req): Json<ResolveMergeRequest>,
) -> ApiResponse<serde_json::Value> {
    let rel_store = async_memory.get_relational_store();
    match rel_store.resolve_pending_event_merge(merge_id, &req.action).await {
        Ok(()) => ApiResponse::success(serde_json::json!({
            "merge_id": merge_id,
            "action": req.action,
            "resolved": true
        })),
        Err(e) => {
            error!("处置待确认合并失败 {}: {}", merge_id, e);
            ApiResponse::error_with_status(
                format!("处理失败: {}", e),
                StatusCode::BAD_REQUEST,
            )
        }
    }
}

/// 搜索联系人
async fn search_contacts(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<SearchContactsRequest>,
) -> ApiResponse<Vec<ContactDetail>> {
    // S-09: 验证 query 长度
    if let Err(e) = validate_query_length(&req.query) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&req.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    let scope = match req.scope_raw.as_deref() {
        None | Some("") => Scope::User,
        Some(s) if s.eq_ignore_ascii_case("user") => Scope::User,
        Some(s) if s.eq_ignore_ascii_case("agent") => Scope::Agent,
        Some(s) => {
            return ApiResponse::error_with_status(
                format!("invalid scope '{}': expected 'user' or 'agent'", s),
                StatusCode::BAD_REQUEST,
            );
        }
    };
    // S-10: 限制 limit 上限
    let limit = cap_limit(req.limit);
    match async_memory.search_contacts(&req.query, limit, scope, req.user_id.as_deref(), req.include_archived, req.include_virtual).await {
        Ok(contacts) => ApiResponse::success(contacts),
        Err(e) => {
            error!("搜索联系人失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 联系人详情查询参数
#[derive(Debug, Deserialize)]
struct ContactDetailQuery {
    /// 操作范围：user / agent
    #[serde(default)]
    scope: Option<String>,
    /// 用户 ID（scope=user 时）
    #[serde(default)]
    user_id: Option<String>,
    /// 返回的事件条数上限
    #[serde(default)]
    event_limit: Option<usize>,
    /// 返回的相关记忆条数上限
    #[serde(default)]
    memory_limit: Option<usize>,
    /// 时间范围起点：`30d` / `4w` / `12h` / `2026-09-30` / RFC3339
    ///
    /// 用于"查某个人近一个月跟我有关的事"这类时间线查询。
    /// 只过滤事件；`memory` 仍按最近对话返回。
    #[serde(default)]
    since: Option<String>,
    /// 时间范围终点，格式同 `since`；只写日期时按当天 23:59:59 处理
    #[serde(default)]
    until: Option<String>,
}

/// 联系人全息详情：人设 + 关联联系人 + 关联事件 + 相关记忆原文
async fn get_contact_detail(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(contact_id): Path<String>,
    Query(q): Query<ContactDetailQuery>,
) -> ApiResponse<serde_json::Value> {
    if let Err(e) = validate_user_id(&q.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    let scope = match q.scope.as_deref() {
        None | Some("") => Scope::User,
        Some(s) if s.eq_ignore_ascii_case("user") => Scope::User,
        Some(s) if s.eq_ignore_ascii_case("agent") => Scope::Agent,
        Some(s) => {
            return ApiResponse::error_with_status(
                format!("invalid scope '{}': expected 'user' or 'agent'", s),
                StatusCode::BAD_REQUEST,
            );
        }
    };
    // 上限保护：联系人详情是聚合查询，不限制条数容易被一个请求拉爆
    let event_limit = cap_limit(q.event_limit.unwrap_or(20)).min(100);
    let memory_limit = cap_limit(q.memory_limit.unwrap_or(10)).min(50);

    match async_memory
        .get_contact_detail(
            &contact_id,
            scope,
            q.user_id.as_deref(),
            event_limit,
            memory_limit,
            q.since.as_deref(),
            q.until.as_deref(),
        )
        .await
    {
        Ok(detail) => ApiResponse::success(detail),
        Err(e) => {
            // 联系人不存在属于"查无此物"，不该报内部错误
            if matches!(e, crate::error::ShouxinError::EntityNotFound(_)) {
                return ApiResponse::error_with_status(
                    format!("联系人不存在: {}", contact_id),
                    StatusCode::NOT_FOUND,
                );
            }
            error!("查询联系人详情失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 搜索事件
async fn search_events(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<SearchEventsRequest>,
) -> ApiResponse<Vec<EventDetail>> {
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&req.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    // 解析时间
    let start_time = req
        .start_time
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let end_time = req
        .end_time
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    // 构建过滤器（S-10: 限制 limit 上限）
    let mut filter = EventFilter::new().with_limit(cap_limit(req.limit));

    if let Some(ref user_id) = req.user_id {
        filter = filter.with_user_id(user_id.clone());
    }
    if let Some(start) = start_time {
        if let Some(end) = end_time {
            filter = filter.with_time_range(start, end);
        }
    }
    if let Some(importance) = req.importance {
        filter = filter.with_importance(importance);
    }
    if let Some(status) = req.status {
        filter = filter.with_status(status);
    }
    if req.include_archived {
        filter = filter.with_include_archived(true);
    }
    if let Some(month) = req.archived_month {
        filter = filter.with_archived_month(Some(month));
    }

    match async_memory.search_events(filter, req.scope, req.user_id.as_deref()).await {
        Ok(events) => ApiResponse::success(events),
        Err(e) => {
            error!("搜索事件失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 获取待确认事件合并列表
async fn get_pending_event_merges(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<GetPendingMergesRequest>,
) -> ApiResponse<Vec<PendingMergeItem>> {
    let owner_type = req.owner_type.as_ref().map(|t| match t.as_str() {
        "User" => crate::core::contact::OwnerType::User,
        "Contact" => crate::core::contact::OwnerType::Contact,
        "Agent" => crate::core::contact::OwnerType::Agent,
        "Public" => crate::core::contact::OwnerType::Public,
        _ => crate::core::contact::OwnerType::User,
    });

    // S-10: 限制 limit 上限
    let limit = cap_limit(req.limit);
    match async_memory.get_pending_event_merges(
        owner_type,
        req.owner_id.as_deref(),
        Some(limit),
    ).await {
        Ok(pending_merges) => {
            let items: Vec<PendingMergeItem> = pending_merges.into_iter().map(|m| {
                PendingMergeItem {
                    id: m.id,
                    source_event_id: m.source_event_id,
                    target_event_id: m.target_event_id,
                    source_event_summary: m.source_event_summary,
                    target_event_summary: m.target_event_summary,
                    similarity_score: m.similarity_score,
                    status: m.status,
                    created_at: m.created_at.to_rfc3339(),
                }
            }).collect();
            ApiResponse::success(items)
        }
        Err(e) => {
            error!("获取待确认合并失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 确认事件合并
async fn confirm_event_merge(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<ConfirmMergeRequest>,
) -> ApiResponse<ConfirmMergeResponse> {
    let action = req.action.as_str();
    if action != "merge" && action != "create" {
        return ApiResponse::error("无效的操作类型，只能是 'merge' 或 'create'".to_string());
    }

    match async_memory.confirm_event_merge(req.merge_id, action, req.source_event_summary.as_deref()).await {
        Ok(event_id) => ApiResponse::success(ConfirmMergeResponse {
            success: true,
            message: if action == "merge" { "事件已合并".to_string() } else { "新事件已创建".to_string() },
            event_id: Some(event_id),
        }),
        Err(e) => {
            error!("确认合并失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 搜索历史对话
async fn search_history(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<SearchHistoryRequest>,
) -> ApiResponse<Vec<Conversation>> {
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&req.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    // 解析时间，如果未提供则使用默认值
    let start_time = match req.start_time {
        Some(ref s) => match DateTime::parse_from_rfc3339(s) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(e) => {
                warn!("无效的 start_time 格式: {}", e);
                return ApiResponse::error("无效的 start_time 格式".to_string());
            }
        },
        None => {
            // 使用一个简单的方式创建1970-01-01 00:00:00 UTC
            use chrono::TimeZone;
            Utc.timestamp_opt(0, 0).unwrap()
        },
    };

    let end_time = match req.end_time {
        Some(ref e) => match DateTime::parse_from_rfc3339(e) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(e) => {
                warn!("无效的 end_time 格式: {}", e);
                return ApiResponse::error("无效的 end_time 格式".to_string());
            }
        },
        None => Utc::now(),
    };

    match async_memory.search_history((start_time, end_time), req.scope, req.user_id.as_deref()).await {
        Ok(history) => ApiResponse::success(history),
        Err(e) => {
            error!("搜索历史对话失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 语义搜索
#[axum::debug_handler]
async fn semantic_search(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<SemanticSearchRequest>,
) -> ApiResponse<Vec<VectorSearchResult>> {
    // S-09: 验证 query 长度
    if let Err(e) = validate_query_length(&req.query) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&req.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    // S-10: 限制 limit 上限
    let limit = cap_limit(req.limit);
    match async_memory.semantic_search(&req.query, limit, req.scope, req.user_id.as_deref(), req.source_types).await {
        Ok(results) => {
            let filtered_results = if let Some(ref conv_id) = req.conversation_id {
                results.into_iter()
                    .filter(|r| r.material.source_conversation_id.as_ref() == Some(conv_id))
                    .collect()
            } else {
                results
            };
            ApiResponse::success(filtered_results)
        }
        Err(e) => {
            error!("语义搜索失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}



/// 学习素材详情查询参数（可选作用域校验）
///
/// 显式传 `scope` 时校验归属；不传时保持向后兼容（仅按 id 返回）。
#[derive(Debug, Deserialize)]
struct MaterialOwnerQuery {
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    user_id: Option<String>,
}

/// 获取学习素材详情
async fn get_learning_material(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(id): Path<String>,
    Query(owner): Query<MaterialOwnerQuery>,
) -> ApiResponse<LearningMaterial> {
    match async_memory.get_learning_material(&id).await {
        Ok(Some(material)) => {
            // F-VS5: 传了 scope 就必须匹配归属，否则按"不存在"处理，避免跨 owner 读取
            if let Some(scope) = owner.scope {
                let expected_type = scope.to_owner_type();
                let expected_id = match scope {
                    Scope::User => owner
                        .user_id
                        .clone()
                        .unwrap_or_else(|| crate::core::owner::DEFAULT_USER_ID.to_string()),
                    Scope::Agent => crate::core::owner::primary_agent_id().to_string(),
                };
                if material.owner_type != expected_type || material.owner_id != expected_id {
                    return ApiResponse::error("Learning material not found".to_string());
                }
            }
            ApiResponse::success(material)
        }
        Ok(None) => ApiResponse::error(
            "Learning material not found".to_string(),
        ),
        Err(e) => {
            error!("获取学习素材失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

// ==================== 待办事项处理器 ====================

/// 添加待办事项
async fn add_todo(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(mut req): Json<AddTodoRequest>,
) -> ApiResponse<TodoItem> {
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&req.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    // B4: 去除标题前后空格
    let trimmed_title = req.title.trim().to_string();
    if trimmed_title.is_empty() {
        return ApiResponse::error_with_status("Title is required".to_string(), StatusCode::BAD_REQUEST);
    }
    req.title = trimmed_title;

    // I5: 超长标题截断（字符级，正确处理多字节 UTF-8）
    let char_count = req.title.chars().count();
    if char_count > MAX_TODO_TITLE_LEN {
        let truncated: String = req.title.chars().take(MAX_TODO_TITLE_LEN).collect();
        tracing::warn!("待办标题过长（{}字符），已截断至 {} 字符", char_count, MAX_TODO_TITLE_LEN);
        req.title = truncated;
    }

    // I4: importance 枚举校验
    if !validate_importance(&req.importance) {
        tracing::warn!(
            "非法的 importance 值: '{}', 降级为默认值 'medium'",
            req.importance
        );
        req.importance = "medium".to_string();
    }

    // 解析截止时间：不填则默认"7 天后 23:59:59"（与抽取链路一致，避免接口强制要求 deadline）
    let deadline = match req.deadline.as_deref().map(str::trim) {
        None | Some("") => crate::utils::default_deadline_in_days(7),
        Some(raw) => match DateTime::parse_from_rfc3339(raw) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(e) => {
                warn!("无效的 deadline 格式: {}", e);
                return ApiResponse::error("无效的截止时间格式".to_string());
            }
        },
    };

    // 使用客户端提供的 ID 或服务端生成
    let todo_id = req.todo_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // 根据scope获取所有者信息（owner_id 统一从配置解析，禁止写死）
    let user_id_opt = req.user_id.clone();
    let (owner_type, owner_id) = match req.scope {
        Scope::User => (
            crate::core::contact::OwnerType::User,
            req.user_id
                .unwrap_or_else(|| crate::core::owner::DEFAULT_USER_ID.to_string()),
        ),
        Scope::Agent => (
            crate::core::contact::OwnerType::Agent,
            crate::core::owner::primary_agent_id().to_string(),
        ),
    };

    // 构建待办事项
    let contact_ids = req.contact_ids.unwrap_or_default();

    if !contact_ids.is_empty() {
        match async_memory
            .validate_contacts(&contact_ids, owner_type, &owner_id)
            .await
        {
            Ok(_) => {}
            Err(e) => {
                error!("验证联系人失败: {}", e);
                return ApiResponse::error("内部错误".to_string());
            }
        }
    }

    // D3: 检查是否已存在相同标题且相同deadline的未完成待办
    match async_memory.list_todos(
        TodoFilter::new().with_status("pending".to_string()),
        req.scope,
        user_id_opt.as_deref(),
    ).await {
        Ok(pending_todos) => {
            for existing in &pending_todos {
                if existing.title == req.title && existing.owner_id == owner_id
                    && format!("{:?}", existing.owner_type) == format!("{:?}", owner_type)
                {
                    let same_deadline = (existing.deadline.timestamp() / 60) == (deadline.timestamp() / 60);
                    if same_deadline {
                        tracing::debug!("待办已存在，拒绝创建: {} (相同title+deadline)", req.title);
                        return ApiResponse::error_with_status(
                            format!("待办已存在: {}", req.title),
                            StatusCode::CONFLICT,
                        );
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("查询已存在待办失败，继续创建: {}", e);
        }
    }

    let mut todo = TodoItem::new(
        todo_id,
        contact_ids,
        owner_id,
        owner_type,
        req.title,
        deadline,
        req.importance,
    );

    if let Some(description) = req.description {
        todo = todo.with_description(description);
    }
    if let Some(source_event_id) = req.source_event_id {
        todo = todo.with_source_event(source_event_id);
    }

    match async_memory.add_todo(&todo).await {
        Ok(_) => ApiResponse::success(todo),
        Err(ShouxinError::ConflictError(msg)) => {
            ApiResponse::error_with_status(msg, StatusCode::CONFLICT)
        }
        Err(e) => {
            error!("添加待办失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 获取待办列表
async fn list_todos(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Query(req): Query<ListTodosRequest>,
) -> ApiResponse<Vec<TodoItem>> {
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&req.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    // S-10: 限制 limit 上限
    let limit = std::cmp::min(req.limit, MAX_SEARCH_LIMIT as i64);
    let filter = TodoFilter::new().with_limit(limit).with_include_archived(req.include_archived);

    let filter = if let Some(ref status) = req.status {
        filter.with_status(status.clone())
    } else {
        filter
    };

    let filter = if let Some(ref contact_id) = req.contact_id {
        filter.with_contact(contact_id.clone())
    } else {
        filter
    };

    match async_memory.list_todos(filter, req.scope, req.user_id.as_deref()).await {
        Ok(todos) => ApiResponse::success(todos),
        Err(e) => {
            error!("获取待办列表失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 获取待办详情
async fn get_todo(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(id): Path<String>,
    Query(params): Query<ScopeQueryParams>,
) -> ApiResponse<TodoItem> {
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&params.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    let (owner_type, owner_id) = resolve_owner(&async_memory, params.scope, params.user_id.as_deref(), &id).await;
    match async_memory.get_todo(&id, owner_type, &owner_id).await {
        Ok(Some(todo)) => ApiResponse::success(todo),
        Ok(None) => ApiResponse::error_with_status(
            "Todo not found".to_string(),
            StatusCode::NOT_FOUND,
        ),
        Err(e) => {
            error!("获取待办详情失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 完成待办事项
async fn complete_todo(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(id): Path<String>,
    Query(params): Query<ScopeQueryParams>,
) -> ApiResponse<()> {
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&params.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    let (owner_type, owner_id) = resolve_owner(&async_memory, params.scope, params.user_id.as_deref(), &id).await;
    match async_memory.complete_todo(&id, owner_type, &owner_id).await {
        Ok(_) => ApiResponse::success(()),
        Err(e) => {
            error!("完成待办失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 删除待办事项
async fn delete_todo(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(id): Path<String>,
    Query(params): Query<ScopeQueryParams>,
) -> ApiResponse<()> {
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&params.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    let (owner_type, owner_id) = resolve_owner(&async_memory, params.scope, params.user_id.as_deref(), &id).await;
    match async_memory.delete_todo(&id, owner_type, &owner_id).await {
        Ok(_) => ApiResponse::success(()),
        Err(e) => {
            error!("删除待办失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

async fn resolve_owner(
    async_memory: &AsyncMemory,
    scope: Option<Scope>,
    user_id: Option<&str>,
    todo_id: &str,
) -> (crate::core::contact::OwnerType, String) {
    if let Some(scope) = scope {
        return scope.to_owner_params(user_id);
    }
    // 未显式指定作用域时，以待办自身的归属为准。
    // 原实现只接受 User，Agent 作用域的待办会被回退成 (User, default_user)，
    // 导致 get/complete/delete 直接查不到（F-T2）。
    if let Ok(Some(owner)) = async_memory.get_todo_owner(todo_id).await {
        return owner;
    }
    // 待办不存在时回退到默认作用域（与 `default_scope()` 一致：agent）
    Scope::Agent.to_owner_params(user_id)
}

// ==================== 心跳模块处理器 ====================

#[derive(Debug, Serialize, Deserialize)]
pub struct AnswerRequest {
    pub question_id: String,
    pub answer: String,
}

/// 触发心跳扫描
async fn trigger_heartbeat(
    Extension(scheduler): Extension<Arc<HeartbeatScheduler>>,
    req: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    // P2-6: 解析 maintenance_only 参数，支持仅执行维护心跳（不调用 LLM）
    let maintenance_only = req
        .map(|Json(v)| v.get("maintenance_only").and_then(|m| m.as_bool()).unwrap_or(false))
        .unwrap_or(false);

    match scheduler.trigger_now(maintenance_only).await {
        Ok(report) => ApiResponse::success(report),
        Err(ShouxinError::ConcurrencyError(msg)) => {
            ApiResponse::error_with_status(msg, StatusCode::CONFLICT)
        }
        Err(e) => {
            error!("触发心跳扫描失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 获取待处理问题
async fn get_pending_questions(
    Extension(scheduler): Extension<Arc<HeartbeatScheduler>>,
) -> ApiResponse<Vec<Question>> {
    match scheduler.question_pool().get_pending_questions(usize::MAX).await {
        Ok(questions) => ApiResponse::success(questions),
        Err(e) => {
            error!("获取待处理问题失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

/// 获取心跳状态
async fn get_heartbeat_status(
    Extension(scheduler): Extension<Arc<HeartbeatScheduler>>,
) -> ApiResponse<HeartbeatStatus> {
    let status = scheduler.get_status().await;
    ApiResponse::success(status)
}

/// 回答问题
async fn answer_question(
    Extension(scheduler): Extension<Arc<HeartbeatScheduler>>,
    Json(req): Json<AnswerRequest>,
) -> ApiResponse<()> {
    tracing::debug!("Received answer for question {}: {}", req.question_id, req.answer);
    if let Err(e) = scheduler.question_pool().resolve_question(&req.question_id, &req.answer).await {
        error!("回答问题失败: {}", e);
        return ApiResponse::error("内部错误".to_string());
    }
    ApiResponse::success(())
}

// ==================== 智能搜索处理器 ====================

/// 智能搜索请求
#[derive(Debug, Deserialize)]
pub struct SmartSearchRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default = "default_search_types")]
    pub types: Vec<String>,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub user_id: Option<String>,
}

/// 智能搜索响应
fn default_search_types() -> Vec<String> {
    vec![
        "contacts".to_string(),
        "events".to_string(),
        "history".to_string(),
        "todos".to_string(),
    ]
}

#[derive(Debug, Serialize)]
pub struct SmartSearchResponse {
    pub contacts: Vec<serde_json::Value>,
    pub events: Vec<serde_json::Value>,
    pub history: Vec<serde_json::Value>,
    pub todos: Vec<serde_json::Value>,
}

/// 智能搜索
async fn smart_search(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<SmartSearchRequest>,
) -> ApiResponse<SmartSearchResponse> {
    // S-09: 验证 query 长度
    if let Err(e) = validate_query_length(&req.query) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    // S-09: 验证 user_id 格式
    if let Err(e) = validate_user_id(&req.user_id) {
        return ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST);
    }
    let mut response = SmartSearchResponse {
        contacts: Vec::new(),
        events: Vec::new(),
        history: Vec::new(),
        todos: Vec::new(),
    };

    // 合理限制每个类型的结果数量，避免超时（S-10: 上限 20，低于 MAX_SEARCH_LIMIT）
    let limit = req.limit.unwrap_or(10);
    let safe_limit = std::cmp::min(limit as usize, 20);
    let query_lower = req.query.to_lowercase();

    for search_type in &req.types {
        match search_type.as_str() {
            "contacts" => {
                if let Ok(contacts) = async_memory.search_contacts(&req.query, safe_limit, req.scope, req.user_id.as_deref(), false, false).await {
                    response.contacts = contacts.into_iter().map(|c| serde_json::to_value(c).unwrap_or_default()).collect();
                }
            }
            "events" => {
                let filter = crate::traits::store_traits::EventFilter::default()
                    .with_search_query(req.query.clone())
                    .with_limit(safe_limit);
                if let Ok(events) = async_memory.search_events(filter, req.scope, req.user_id.as_deref()).await {
                    response.events = events.into_iter().map(|e| serde_json::to_value(e).unwrap_or_default()).collect();
                }
            }
            "todos" => {
                let filter = crate::api::TodoFilter::new().with_limit(safe_limit as i64);
                if let Ok(todos) = async_memory.list_todos(filter, req.scope, req.user_id.as_deref()).await {
                    // 在内存中过滤待办事项（因为API不支持关键词搜索）
                    let filtered_todos: Vec<_> = todos.into_iter()
                        .filter(|t| {
                            t.title.to_lowercase().contains(&query_lower) ||
                            t.description.as_ref().is_some_and(|d| d.to_lowercase().contains(&query_lower))
                        })
                        .take(safe_limit)
                        .collect();
                    response.todos = filtered_todos.into_iter().map(|t| serde_json::to_value(t).unwrap_or_default()).collect();
                }
            }
            "history" => {
                let time_range = (
                    Utc::now().checked_sub_signed(chrono::Duration::days(30)).unwrap(),
                    Utc::now(),
                );
                if let Ok(history) = async_memory.search_history(time_range, req.scope, req.user_id.as_deref()).await {
                    // 在内存中过滤历史记录（因为API不支持关键词搜索）
                    let filtered_history: Vec<_> = history.into_iter()
                        .filter(|h| h.content.to_lowercase().contains(&query_lower))
                        .take(safe_limit)
                        .collect();
                    response.history = filtered_history.into_iter().map(|h| serde_json::to_value(h).unwrap_or_default()).collect();
                }
            }
            _ => {}
        }
    }

    ApiResponse::success(response)
}

// ==================== 虚拟角色管理 API ====================

use crate::stores::relational::{VirtualCharacterInfo, VirtualCharacterAlias};
use crate::core::contact::OwnerType;

#[derive(Debug, Deserialize)]
pub struct AddVirtualCharacterRequest {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<AliasInput>,
    /// 出自哪部作品 / 哪个体系（历史人物可填朝代或史书）
    #[serde(default)]
    pub source_work: Option<String>,
    /// 原著设定（写死的事实）：身份、能力、关系等，JSON 字符串或纯文本
    #[serde(default)]
    pub canon: Option<String>,
    /// 扮演用的人设（说话风格等）
    #[serde(default)]
    pub persona: Option<String>,
    /// myth / history / anime / game / novel / other
    #[serde(default)]
    pub category: Option<String>,
    /// 归属用户（不填则 default_user）
    ///
    /// 之前这里写死 "default_user"，导致多用户场景下虚拟角色在别人的
    /// 列表里根本查不到（跨 owner 隔离把创建出来的记录挡在外面）。
    #[serde(default)]
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AliasInput {
    pub name: String,
    pub source_work: Option<String>,
    pub alias_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateVirtualCharacterRequest {
    pub name: Option<String>,
    pub aliases: Option<Vec<AliasInput>>,
    #[serde(default)]
    pub source_work: Option<String>,
    #[serde(default)]
    pub canon: Option<String>,
    #[serde(default)]
    pub persona: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
}

/// 虚拟角色响应
///
/// 与联系人响应的关键区别：角色有 `source_work` / `canon`（原著设定，写死的事实），
/// 没有「画像」这个概念——画像会随对话演化，角色设定不会。
#[derive(Debug, Serialize)]
pub struct VirtualEntityResponse {
    pub entity_id: String,
    pub name: String,
    pub aliases: Vec<AliasResponse>,
    pub source_work: Option<String>,
    pub category: String,
    pub canon: Option<String>,
    pub persona: Option<String>,
    pub is_builtin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AliasResponse {
    pub name: String,
    pub source_work: Option<String>,
    pub alias_type: String,
}

impl From<&VirtualCharacterAlias> for AliasResponse {
    fn from(alias: &VirtualCharacterAlias) -> Self {
        AliasResponse {
            name: alias.alias_name.clone(),
            source_work: alias.source_work.clone(),
            alias_type: alias.alias_type.clone(),
        }
    }
}

impl From<&VirtualCharacterInfo> for VirtualEntityResponse {
    fn from(info: &VirtualCharacterInfo) -> Self {
        VirtualEntityResponse {
            entity_id: info.character.id.clone(),
            name: info.character.name.clone(),
            aliases: info.aliases.iter().map(AliasResponse::from).collect(),
            source_work: info.character.source_work.clone(),
            category: info.character.category.clone(),
            canon: info.character.canon.clone(),
            persona: info.character.persona.clone(),
            is_builtin: info.character.is_builtin,
            created_at: info.character.created_at,
            updated_at: info.character.updated_at,
        }
    }
}

async fn list_virtual_entities(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
) -> ApiResponse<Vec<VirtualEntityResponse>> {
    let rel_store = async_memory.get_relational_store();
    match rel_store.virtual_alias_store().list(None, false, 2000).await {
        Ok(entities) => {
            let responses: Vec<VirtualEntityResponse> =
                entities.iter().map(VirtualEntityResponse::from).collect();
            ApiResponse::success(responses)
        }
        Err(e) => {
            error!("获取虚拟角色列表失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

async fn get_virtual_entity(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(character_id): Path<String>,
) -> ApiResponse<Option<VirtualEntityResponse>> {
    let rel_store = async_memory.get_relational_store();
    match rel_store.virtual_alias_store().get_by_id(&character_id).await {
        Ok(entity) => {
            let response = entity.as_ref().map(VirtualEntityResponse::from);
            ApiResponse::success(response)
        }
        Err(e) => {
            error!("获取虚拟角色失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}

async fn add_virtual_entity(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<AddVirtualCharacterRequest>,
) -> ApiResponse<VirtualEntityResponse> {
    use crate::stores::relational::NewVirtualCharacter;

    let rel_store = async_memory.get_relational_store();
    let aliases: Vec<VirtualCharacterAlias> = req
        .aliases
        .iter()
        .map(|a| {
            VirtualCharacterAlias::new(
                String::new(), // create() 内部填
                a.name.clone(),
                a.source_work.clone().or_else(|| req.source_work.clone()),
                a.alias_type.clone(),
            )
        })
        .collect();

    let input = NewVirtualCharacter {
        name: req.name.clone(),
        source_work: req.source_work.clone(),
        canon: req.canon.clone(),
        persona: req.persona.clone(),
        category: req.category.clone(),
        owner_id: req.user_id.clone(),
        is_builtin: false,
        aliases,
    };

    match rel_store.virtual_alias_store().create(input).await {
        Ok(info) => ApiResponse::success(VirtualEntityResponse::from(&info)),
        Err(e) => {
            error!("创建虚拟角色失败: {}", e);
            ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST)
        }
    }
}

async fn update_virtual_entity(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(character_id): Path<String>,
    Json(req): Json<UpdateVirtualCharacterRequest>,
) -> ApiResponse<VirtualEntityResponse> {
    let rel_store = async_memory.get_relational_store();
    let store = rel_store.virtual_alias_store();

    match store
        .update(
            &character_id,
            req.name.clone(),
            req.source_work.clone(),
            req.canon.clone(),
            req.persona.clone(),
            req.category.clone(),
        )
        .await
    {
        Ok(Some(info)) => {
            if let Some(alias_inputs) = &req.aliases {
                let aliases: Vec<VirtualCharacterAlias> = alias_inputs
                    .iter()
                    .map(|a| {
                        VirtualCharacterAlias::new(
                            character_id.clone(),
                            a.name.clone(),
                            a.source_work.clone().or_else(|| req.source_work.clone()),
                            a.alias_type.clone(),
                        )
                    })
                    .collect();
                if let Err(e) = store.replace_aliases(&character_id, &aliases).await {
                    warn!("替换角色别名失败: {}", e);
                }
            }
            match store.get_by_id(&character_id).await {
                Ok(Some(info)) => ApiResponse::success(VirtualEntityResponse::from(&info)),
                _ => ApiResponse::success(VirtualEntityResponse::from(&info)),
            }
        }
        Ok(None) => ApiResponse::error_with_status("虚拟角色不存在".to_string(), StatusCode::NOT_FOUND),
        Err(e) => {
            error!("更新虚拟角色失败: {}", e);
            ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST)
        }
    }
}

async fn delete_virtual_entity(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(character_id): Path<String>,
) -> ApiResponse<serde_json::Value> {
    let rel_store = async_memory.get_relational_store();
    match rel_store.virtual_alias_store().delete(&character_id).await {
        Ok(true) => ApiResponse::success(serde_json::json!({ "deleted": true })),
        Ok(false) => ApiResponse::error_with_status("虚拟角色不存在".to_string(), StatusCode::NOT_FOUND),
        Err(e) => {
            error!("删除虚拟角色失败: {}", e);
            ApiResponse::error_with_status(e.to_string(), StatusCode::BAD_REQUEST)
        }
    }
}

async fn check_virtual_entity_match(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Path(name): Path<String>,
) -> ApiResponse<Option<VirtualEntityResponse>> {
    let rel_store = async_memory.get_relational_store();
    match rel_store.check_virtual_character_match(&name).await {
        Ok(info) => {
            let response = info.as_ref().map(VirtualEntityResponse::from);
            ApiResponse::success(response)
        }
        Err(e) => {
            error!("检查虚拟角色匹配失败: {}", e);
            ApiResponse::error("内部错误".to_string())
        }
    }
}
