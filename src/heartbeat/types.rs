use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HeartbeatAction {
    AskUser(Question),
    UpdateContactSummary { contact_id: String, new_summary: String },
    MarkEventCompleted { event_id: String, completed_at: DateTime<Utc> },
    MergeEntities { canonical_id: String, alias_ids: Vec<String> },
    CleanupPendingEvents { event_ids: Vec<String> },
    UpdateTodoStatus { todo_id: String, status: String },
    ResolveAmbiguity { ambiguity_id: String, answer: String },
    NoOp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub priority: QuestionPriority,
    pub category: QuestionCategory,
    pub title: String,
    pub detail: String,
    pub context: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub related_contact_ids: Vec<String>,
    pub related_event_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum QuestionPriority {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum QuestionCategory {
    ContactClarification,
    RelationshipInference,
    EventConfirmation,
    ConflictResolution,
    EntityDisambiguation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatReport {
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub actions_generated: usize,
    pub questions_created: usize,
    pub contacts_updated: usize,
    pub events_cleaned: usize,
    pub events_merged: usize,
    pub contacts_merged: usize,
    pub ambiguities_detected: usize,
    pub profiles_updated: usize,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub duration_ms: u64,
    /// 是否仅为维护心跳（不含LLM任务）
    #[serde(default)]
    pub maintenance_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbiguityInfo {
    pub ambiguity_id: String,
    pub ambiguity_type: AmbiguityType,
    pub entity_type: EntityType,
    pub entity_id: String,
    pub description: String,
    pub context_summary: String,
    pub conflicting_statements: Vec<ConflictingStatement>,
    pub related_contact_ids: Vec<String>,
    pub related_event_ids: Vec<String>,
    pub todo_id: Option<String>,
    pub status: AmbiguityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AmbiguityType {
    FactualContradiction,
    PronounAmbiguity,
    RelationUncertain,
    UncertainInformation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntityType {
    Contact,
    Event,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictingStatement {
    pub statement: String,
    pub source_conversation_id: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AmbiguityStatus {
    Pending,
    Resolved,
    Dismissed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactProfile {
    pub contact_id: String,
    pub personal_info: PersonalInfo,
    pub social_relations: Vec<SocialRelation>,
    pub major_events: Vec<MajorEvent>,
    pub objective_evaluation: String,
    pub potential_relations: Vec<PotentialRelation>,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalInfo {
    pub name: String,
    pub aliases: Vec<String>,
    pub known_occupation: Option<String>,
    pub known_age: Option<String>,
    pub known_interests: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialRelation {
    pub relation_type: String,
    pub related_contact_id: Option<String>,
    pub related_contact_name: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MajorEvent {
    pub event_id: String,
    pub summary: String,
    pub timestamp: DateTime<Utc>,
    pub importance: String,
    pub sentiment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PotentialRelation {
    pub target_contact_id: String,
    pub target_contact_name: String,
    pub inferred_relation: String,
    pub confidence: f32,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingState {
    pub last_processed_ts: DateTime<Utc>,
    pub last_profile_update_ts: DateTime<Utc>,
    pub heartbeat_count: u64,
    pub consecutive_skips: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMergeCandidate {
    pub event1_id: String,
    pub event2_id: String,
    pub time_diff_hours: f64,
    pub similarity_score: f32,
    pub shared_keywords: Vec<String>,
    pub shared_contact_ratio: f32,
    pub merge_reason: String,
    pub is_auto_merge: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactMergeCandidate {
    pub primary_contact_id: String,
    pub merged_contact_id: String,
    pub shared_aliases: Vec<String>,
    pub shared_event_ratio: f32,
    pub confidence_score: f32,
    pub merge_type: MergeType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeType {
    AliasOverlap,
    EventCorrelation,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileUpdateLog {
    pub contact_id: String,
    pub date: String,
    pub update_count: u32,
    pub last_update: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryProcessingStats {
    pub conversations_processed: usize,
    pub contacts_created: usize,
    pub contacts_updated: usize,
    pub events_created: usize,
    pub events_merged: usize,
    pub todos_created: usize,
    pub ambiguities_detected: usize,
    pub profiles_updated: usize,
    pub processing_time_ms: u64,
}
