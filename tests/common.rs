use std::sync::Arc;
use shouxin::memory::three_library_manager::ThreeLibraryMemoryManager;
use shouxin::service::file_processor::FileProcessor;
use shouxin::stores::dictionary::SledDictionaryStore;
use shouxin::stores::relational::{RelationalStore, FileStore};
use shouxin::stores::vector::factory::{VectorStoreConfig, VectorStoreFactory};
use shouxin::memory_extractor::DataDispatcher;
use shouxin::memory_extractor::SimpleExtractor;
use shouxin::llm::factory::build_llm_manager;
use shouxin::config::settings::LlmSettings;
use shouxin::embedder::FastEmbedder;
use shouxin::traits::embedder::Embedder;
use shouxin::traits::vector_store::VectorStore;
use tempfile::TempDir;

/// 解析 fastembed 模型缓存目录
///
/// 优先级：
/// 1. `FASTEMBED_CACHE_DIR` 环境变量（CI 或自定义目录）
/// 2. 项目根目录下的 `.fastembed_cache`
///
/// 原实现硬编码了历史工作区绝对路径 `D:\OpenClaw-Workspace\...`（C1），
/// 在其它机器/工作区下会导致离线模式初始化失败。
fn fastembed_cache_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("FASTEMBED_CACHE_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return std::path::PathBuf::from(dir);
        }
    }
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".fastembed_cache")
}

pub struct TestFileComponents {    pub file_store: FileStore,
    pub dictionary: Arc<SledDictionaryStore>,
    pub vector_store: Arc<dyn VectorStore>,
    pub embedder: Arc<dyn Embedder>,
    pub relational: Arc<RelationalStore>,
    pub _temp_dir: TempDir,
}

pub async fn create_test_file_components() -> Result<TestFileComponents, Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;

    let fastembed_cache = fastembed_cache_dir();
    let hf_cache = fastembed_cache.join("huggingface").join("hub");
    std::env::set_var("HF_HUB_CACHE", hf_cache.to_string_lossy().to_string());
    std::env::set_var("HF_HUB_OFFLINE", "1");
    std::env::set_var("HF_HUB_DISABLE_SYMLINKS", "1");
    std::env::set_var("FASTEMBED_CACHE_DIR", fastembed_cache.to_string_lossy().to_string());

    let db_path = temp_dir.path().join("test_file.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());

    let pool = sqlx::SqlitePool::connect(&db_url).await?;
    let relational = RelationalStore::new(pool);
    relational.init_schema().await?;
    let file_store = relational.file_store().clone();
    let relational = Arc::new(relational);

    let dictionary_path = temp_dir.path().join("sled_file_dict");
    let dictionary = SledDictionaryStore::open(dictionary_path.to_str().unwrap())?;
    let dictionary = Arc::new(dictionary);

    let vector_path = temp_dir.path().join("vector_file_store");
    let vector_config = VectorStoreConfig {
        path: vector_path.to_string_lossy().to_string(),
        store_type: Default::default(),
        dimension: 768,
        index_config: Default::default(),
    };
    let vector = VectorStoreFactory::create_embedded(&vector_config)?;
    let vector: Arc<dyn VectorStore> = vector;

    let embedder = FastEmbedder::new("intfloat/multilingual-e5-base")?;
    let embedder = Arc::new(embedder);

    Ok(TestFileComponents {
        file_store,
        dictionary,
        vector_store: vector,
        embedder,
        relational,
        _temp_dir: temp_dir,
    })
}

pub fn make_file_processor(comps: &TestFileComponents) -> FileProcessor {
    FileProcessor::new(
        comps.file_store.clone(),
        comps.dictionary.clone(),
        comps.vector_store.clone(),
        comps.embedder.clone(),
    )
}

pub async fn create_test_manager() -> Result<ThreeLibraryMemoryManager, Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;

    let fastembed_cache = fastembed_cache_dir();
    let hf_cache = fastembed_cache.join("huggingface").join("hub");
    std::env::set_var("HF_HUB_CACHE", hf_cache.to_string_lossy().to_string());
    std::env::set_var("HF_HUB_OFFLINE", "1");
    std::env::set_var("HF_HUB_DISABLE_SYMLINKS", "1");
    std::env::set_var("FASTEMBED_CACHE_DIR", fastembed_cache.to_string_lossy().to_string());

    let db_path = temp_dir.path().join("test.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());

    let pool = sqlx::SqlitePool::connect(&db_url).await?;
    let relational = RelationalStore::new(pool);
    relational.init_schema().await?;
    let relational = Arc::new(relational);

    let dictionary_path = temp_dir.path().join("sled_dict");
    let dictionary = SledDictionaryStore::open(dictionary_path.to_str().unwrap())?;
    let dictionary = Arc::new(dictionary);

    let vector_path = temp_dir.path().join("vector_store");
    let vector_config = VectorStoreConfig {
        path: vector_path.to_string_lossy().to_string(),
        store_type: Default::default(),
        dimension: 768,
        index_config: Default::default(),
    };
    let vector = VectorStoreFactory::create_embedded(&vector_config)?;

    let embedder = FastEmbedder::new("intfloat/multilingual-e5-base")?;
    let embedder = Arc::new(embedder);

    let settings = LlmSettings::default();
    let llm_manager = build_llm_manager(&settings).await?;
    let llm_client = Arc::from(llm_manager);

    let extractor = Arc::new(SimpleExtractor);

    let data_dispatcher = DataDispatcher::new(
        dictionary.clone(),
        relational.clone(),
        vector.clone(),
    );
    let data_dispatcher = Arc::new(data_dispatcher);

    let manager = ThreeLibraryMemoryManager::new(
        dictionary,
        relational,
        vector,
        llm_client,
        embedder,
        extractor,
        data_dispatcher,
    );

    Ok(manager)
}