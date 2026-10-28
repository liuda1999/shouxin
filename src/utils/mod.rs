//! 工具模块
//!
//! 提供通用工具函数和辅助方法

pub mod event_meta;
pub mod time_parser;
pub mod validation;

pub use event_meta::{analyze_sentiment, classify_importance};
pub use time_parser::parse_event_time;
pub use validation::{validate_finite, validate_non_empty, validate_range, validate_vector};
