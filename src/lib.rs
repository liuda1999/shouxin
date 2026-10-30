//! 守心（Shouxin）Rust 库
//! 
//! 提供完整的内存管理、向量存储、图数据库和LLM集成功能
//! 所有操作均使用异步方式，确保高性能和可靠性

pub mod api;
pub mod config;
pub mod core;
pub mod embedder;
pub mod error;
pub mod heartbeat;
pub mod initialization;
pub mod learning;
pub mod llm;
pub mod memory;
pub mod memory_extractor;
pub mod service;
pub mod stores;
pub mod testing;
pub mod traits;
pub mod utils;

pub use core::{
    Conversation, Embedding, Fact, FactCollection, FactSource, Memory, MemoryAction, MemoryFilter,
    MemoryState, MemoryType, Message, MessageRole, SearchQuery, SearchResult, SearchResults,
    SearchSource,
};
pub use embedder::{EmbedderFactory, EmbedderType, FastEmbedder};
pub use error::ShouxinError;
pub use llm::{build_llm_manager, LlmClientManager, ManagerStatus, OllamaLlmClient, ApiClient, LlmSourceType};

pub use api::{AsyncMemory, SyncMemory};
pub use memory::{ThreeLibraryMemoryConfig, ThreeLibraryMemoryManager};
pub use traits::{
    DistanceMetric, Embedder, EmbedderConfig, LlmClient, LlmConfig, SyncEmbedder, VectorStore,
    VectorStoreConfig,
};

pub use stores::relational::{TodoItem, TodoListStore};
pub use stores::{
    ContactIndexStore, ContactsTotalStore, DictionaryStore as StoresDictionaryStore,
    EventIndexStore, MaintenanceService, RelationalStore as StoresRelationalStore,
};

pub use memory_extractor::{
    DataDispatcher, DataDispatcherTrait, DispatchInput, ElementType, ExtractedElements,
    ExtractedEvent, ExtractedLearning, ExtractedPerson, MemoryExtractor as MemoryElementsExtractor,
    SimpleExtractor,
};

pub use traits::store_traits::{
    ArchiveRecord as TraitsArchiveRecord, ArchiveType as TraitsArchiveType,
    CleanupRule as TraitsCleanupRule, ContactIndex as TraitsContactIndex,
    ContactRecord as TraitsContactRecord, EventIndex as TraitsEventIndex,
};

pub use initialization::{InitializationHelper, SystemConfig, SystemInitializer};

pub use learning::{
    EntityAlias, EntityMention, EntityNormalizer, ErrorEntry, FeatureRequestEntry,
    KnowledgeGraphExtractor, KnowledgeTriple, LearningEntry, LearningLogger, LogType,
    SliceRelation, SliceRelationsBuilder, SummaryResult, Summarizer,
};

pub use stores::vector::factory::VectorStoreType;
pub use stores::vector::store::EmbeddedVectorStore;

/// 公共API入口点
pub struct Shouxin {
    // 核心组件
}

impl Shouxin {
    /// 创建一个新的Shouxin实例
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Shouxin {
    fn default() -> Self {
        Self::new()
    }
}
