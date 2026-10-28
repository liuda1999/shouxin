use axum::{
    async_trait,
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use constant_time_eq::constant_time_eq;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::core::contact::{ContactIndex, ContactTotal, OwnerType};
use crate::stores::relational::store::RelationalStore;
use chrono::Utc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistrationRequest {
    pub agent_id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistrationResponse {
    pub success: bool,
    pub agent_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalEventAddRequest {
    pub description: String,
    pub related_contacts: Vec<String>,
    pub source_agent_id: String,
    #[serde(default = "default_owner_type")]
    pub owner_type: String,
    #[serde(default = "default_owner_id")]
    pub owner_id: String,
}

fn default_owner_type() -> String {
    "Agent".to_string()
}
/// 内部 API 未显式传 owner_id 时的默认值
///
/// 必须与 `settings.agent.primary_agent_id` 一致（F-T1）。
fn default_owner_id() -> String {
    crate::core::owner::primary_agent_id().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalEventAddResponse {
    pub success: bool,
    pub event_id: Option<String>,
    pub message: String,
}

// 内部 API 认证中间件
pub struct InternalTokenAuth {
    pub token: String,
}

#[derive(Debug)]
pub struct InternalAuthRejection {
    status: StatusCode,
    message: &'static str,
}

impl IntoResponse for InternalAuthRejection {
    fn into_response(self) -> axum::response::Response {
        let body = serde_json::json!({
            "success": false,
            "error": self.message
        });
        (self.status, Json(body)).into_response()
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for InternalTokenAuth
where
    S: Send + Sync,
{
    type Rejection = InternalAuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok());

        let internal_token = match std::env::var("INTERNAL_API_TOKEN") {
            Ok(token) if !token.is_empty() => token,
            _ => {
                return Err(InternalAuthRejection {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "INTERNAL_API_TOKEN not configured",
                });
            }
        };

        match auth_header {
            Some(header) if header.starts_with("Bearer ") => {
                let token = header.trim_start_matches("Bearer ").to_string();
                if constant_time_eq(token.as_bytes(), internal_token.as_bytes()) {
                    Ok(InternalTokenAuth { token })
                } else {
                    Err(InternalAuthRejection {
                        status: StatusCode::UNAUTHORIZED,
                        message: "Invalid internal token",
                    })
                }
            }
            _ => Err(InternalAuthRejection {
                status: StatusCode::UNAUTHORIZED,
                message: "Authorization header missing",
            }),
        }
    }
}

// 内部 API 路由状态
#[derive(Clone)]
pub struct InternalApiState {
    pub relational_store: Arc<RelationalStore>,
    pub primary_agent_id: String,
}

impl InternalApiState {
    pub fn new(relational_store: RelationalStore, primary_agent_id: String) -> Self {
        Self {
            relational_store: Arc::new(relational_store),
            primary_agent_id,
        }
    }
}

// 注册子智能体处理函数
async fn register_agent(
    _auth: InternalTokenAuth,
    State(state): State<InternalApiState>,
    Json(req): Json<AgentRegistrationRequest>,
) -> impl IntoResponse {
    let now = Utc::now();

    let existing = state
        .relational_store
        .get_contact_by_id(&req.agent_id, OwnerType::Agent, &state.primary_agent_id)
        .await;

    match existing {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                Json(AgentRegistrationResponse {
                    success: false,
                    agent_id: req.agent_id,
                    message: "Agent already exists".to_string(),
                }),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AgentRegistrationResponse {
                    success: false,
                    agent_id: req.agent_id,
                    message: format!("Failed to check agent existence: {}", e),
                }),
            );
        }
        Ok(None) => {}
    }

    let contact_total = ContactTotal {
        id: req.agent_id.clone(),
        name: req.name.clone(),
        aliases: vec![],
        is_self: false,
        created_at: now,
        last_interaction_at: now,
        related_contact_ids: vec![],
        relationship_tags: vec![],
        relation_weight: None,
        background: req.description,
        notes: None,
        metadata: Default::default(),
        owner_type: OwnerType::Agent,
        owner_id: state.primary_agent_id.clone(),
        is_agent: true,
        parent_agent_id: Some(state.primary_agent_id.clone()),
        is_virtual: false,
    };

    let contact_index = ContactIndex::from_total(&contact_total);

    match state.relational_store.upsert_contact_index(&contact_index).await {
        Ok(_) => (
            StatusCode::CREATED,
            Json(AgentRegistrationResponse {
                success: true,
                agent_id: req.agent_id,
                message: "Sub-agent registered successfully".to_string(),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AgentRegistrationResponse {
                success: false,
                agent_id: req.agent_id,
                message: format!("Failed to register agent: {}", e),
            }),
        ),
    }
}

// 添加内部事件处理函数
async fn add_internal_event(
    _auth: InternalTokenAuth,
    State(state): State<InternalApiState>,
    Json(req): Json<InternalEventAddRequest>,
) -> impl IntoResponse {
    let owner_type = match req.owner_type.as_str() {
        "Agent" => OwnerType::Agent,
        "User" => OwnerType::User,
        _ => OwnerType::Agent,
    };
    let owner_id = req.owner_id.clone();

    let event_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();
    let contact_ids_json = serde_json::to_string(&req.related_contacts).unwrap_or_else(|_| "[]".to_string());
    let event_parsed_time = crate::utils::parse_event_time(&req.description, now);

    let mut event_index = crate::core::EventIndex {
        event_id: event_id.clone(),
        contact_ids: contact_ids_json,
        owner_id,
        owner_type,
        time_range_start: now,
        time_range_end: now,
        importance: "medium".to_string(),
        status: "pending".to_string(),
        created_at: now,
        completed_at: None,
        summary: req.description.clone(),
        title: None,
        event_type: String::new(),
        deadline: None,
        auto_invalid_at: None,
        is_auto_invalid: false,
        is_merged: false,
        merge_target_id: None,
        last_merge_check_at: None,
        event_time: event_parsed_time,
        importance_score: 0.5,
        is_archived: false,
        archived_at: None,
    };
    event_index.compute_auto_invalid_at();

    match state.relational_store.insert_event_index(&event_index).await {
        Ok(_) => (
            StatusCode::CREATED,
            Json(InternalEventAddResponse {
                success: true,
                event_id: Some(event_id),
                message: "Internal event added successfully".to_string(),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(InternalEventAddResponse {
                success: false,
                event_id: None,
                message: format!("Failed to add event: {}", e),
            }),
        ),
    }
}

// 创建内部 API 路由器
pub fn create_internal_api_router(state: InternalApiState) -> Router {
    Router::new()
        .route("/agent/register", post(register_agent))
        .route("/event/add", post(add_internal_event))
        .with_state(state)
}
