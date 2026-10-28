//! 三库统一管理模块
//!
//! 包含字典库、关系库、向量库的统一管理

pub mod dictionary;
pub mod relational;
pub mod vector;

pub use dictionary::{
    ContactsTotalStore, ConversationRecord, ConversationStore, DictionaryStore, SledDictionaryStore,
};
pub use relational::{
    CleanupResult, ContactBackground, ContactBackgroundSummarizer, ContactIndexStore,
    EventIndexStore, MaintenanceService, MaintenanceState, RelationalStore,
};
pub use vector::VectorStoreWrapper;
