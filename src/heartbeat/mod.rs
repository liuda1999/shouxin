//! 心跳模块
//!
//! 负责在系统空闲或定时触发时，对记忆库进行深度整理
//!
//! 包括：
//! - 主动记忆整理
//! - 联系人别名匹配与维护
//! - 事件去重与合并
//! - 歧义检测与待办生成
//! - 联系人画像总结
//! - 资源感知调度

pub mod types;
pub mod question_pool;
pub mod analyzer;
pub mod executor;
pub mod scheduler;

pub use types::{
    HeartbeatAction, Question, QuestionPriority, QuestionCategory, HeartbeatReport,
    AmbiguityInfo, AmbiguityType, AmbiguityStatus, ContactProfile, ProcessingState,
    EventMergeCandidate, ContactMergeCandidate, ProfileUpdateLog, MemoryProcessingStats,
};
pub use question_pool::QuestionPool;
pub use analyzer::HeartbeatAnalyzer;
pub use executor::HeartbeatExecutor;
pub use scheduler::{HeartbeatScheduler, SchedulerConfig, HeartbeatStatus};
