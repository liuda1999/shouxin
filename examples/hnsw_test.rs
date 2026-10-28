//! HNSW 索引功能测试
//!
//! 测试 HNSW 向量库的写入和检索功能

use std::sync::Arc;
use shouxin::memory::three_library_manager::ThreeLibraryMemoryManager;
use shouxin::stores::dictionary::SledDictionaryStore;
use shouxin::stores::relational::RelationalStore;
use shouxin::stores::vector::factory::{VectorStoreConfig, VectorStoreFactory, VectorStoreType};
use shouxin::memory_extractor::{DataDispatcher, RegexExtractor};
use shouxin::llm::LlmFactory;
use shouxin::embedder::FastEmbedder;
use tempfile::TempDir;

async fn create_hnsw_test_manager() -> Result<ThreeLibraryMemoryManager, Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;

    let fastembed_cache = std::path::PathBuf::from("D:\\OpenClaw-Workspace\\ShouXin_Complete_v0.2.0\\.fastembed_cache");
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
        store_type: VectorStoreType::Hnsw,
        dimension: 768,
        index_config: Default::default(),
    };
    let vector = VectorStoreFactory::create(&vector_config)?;

    let llm_client = LlmFactory::create_default()?;
    let llm_client = Arc::from(llm_client);

    let embedder = FastEmbedder::new("intfloat/multilingual-e5-base")?;
    let embedder = Arc::new(embedder);

    let extractor = RegexExtractor::new();

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
        Arc::new(extractor),
        data_dispatcher,
    );

    Ok(manager)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("===========================================");
    println!("HNSW 索引功能测试");
    println!("===========================================\n");

    println!("步骤 1: 初始化 HNSW 向量库管理器...");
    let manager = create_hnsw_test_manager().await?;
    println!("✓ HNSW 向量库初始化成功\n");

    println!("步骤 2: 写入真实记忆数据...");
    println!("-------------------------------------------");

    let test_memories = vec![
        ("Rust 是一种系统级编程语言，性能优异，适合开发高性能应用", "user_001"),
        ("Python 编程语言简单易学，广泛用于数据科学和机器学习领域", "user_001"),
        ("JavaScript 是网页开发的主要语言，可以创建交互式网页效果", "user_001"),
        ("我喜欢在周末去咖啡馆读书，享受安静的阅读时光", "user_002"),
        ("项目管理需要良好的沟通能力和时间管理技巧", "user_002"),
        ("人工智能技术正在改变我们的生活方式和工作模式", "user_001"),
        ("数据库设计需要考虑数据完整性、安全性和查询性能", "user_003"),
        ("云计算提供了弹性的计算资源，降低了企业IT成本", "user_003"),
    ];

    let mut memory_ids = Vec::new();
    for (content, user_id) in &test_memories {
        println!("写入记忆: {}", content);
        let result = manager.add(content, Some(user_id)).await?;
        println!("  → 添加成功 (对话ID: {:?})", result.conversation_id);
    }
    println!("✓ 成功写入 {} 条记忆\n", test_memories.len());

    println!("步骤 3: 执行多次语义检索测试...");
    println!("--------------------------------------------\n");

    let search_queries = vec![
        ("编程语言", "测试1: 搜索编程语言相关记忆"),
        ("Rust", "测试2: 搜索特定编程语言 Rust"),
        ("人工智能", "测试3: 搜索 AI 相关内容"),
        ("读书", "测试4: 搜索阅读相关记忆"),
        ("云计算", "测试5: 搜索云计算相关内容"),
        ("数据库", "测试6: 搜索数据库相关记忆"),
    ];

    for (query, description) in search_queries {
        println!("\n{}", description);
        println!("查询词: \"{}\"", query);
        println!("{}", "-".repeat(50));

        let results = manager.semantic_search(query, 5).await?;

        if results.is_empty() {
            println!("⚠ 未找到相关记忆");
        } else {
            println!("找到 {} 个相关记忆:", results.len());
            for (i, result) in results.iter().enumerate() {
                let content_preview = if result.material.content.len() > 50 {
                    format!("{}...", &result.material.content[..50])
                } else {
                    result.material.content.clone()
                };
                println!("  {}. [分数: {:.4}] {}", i + 1, result.score, content_preview);
            }
        }
    }

    println!("\n===========================================");
    println!("HNSW 索引功能测试完成！");
    println!("===========================================");

    Ok(())
}
