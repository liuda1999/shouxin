//! 工具模块
//!
//! 提供通用工具函数和辅助方法

pub mod cpu_budget;
pub mod event_meta;
pub mod merge_thresholds;
pub mod time_parser;
pub mod validation;

pub use merge_thresholds::{merge_thresholds, set_merge_thresholds};

pub use event_meta::{analyze_sentiment, classify_importance};
pub use time_parser::{
    default_deadline_from, default_deadline_in_days, parse_event_time, resolve_deadline_ex,
    resolve_event_time, try_resolve_event_time, ResolvedDeadline,
};
// end_of_day 是 crate 内工具，不对外导出
pub(crate) use time_parser::end_of_day;
pub use validation::{validate_finite, validate_non_empty, validate_range, validate_vector};
