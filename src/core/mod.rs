//! 核心模块
//!
//! 包含 Shouxin 的核心数据结构定义

pub mod contact;
pub mod embedding;
pub mod fact;
pub mod memory;
pub mod message;
pub mod owner;
pub mod search_result;

pub use contact::{ContactIndex, ContactPatch, ContactTotal, EventIndex};
pub use embedding::Embedding;
pub use fact::{Fact, FactCollection, FactSource};
pub use memory::{Memory, MemoryAction, MemoryFilter, MemoryState, MemoryType};
pub use message::{Conversation, Message, MessageRole};
pub use owner::{default_user_id, init_primary_agent_id, primary_agent_id};
pub use search_result::{SearchQuery, SearchResult, SearchResults, SearchSource};
