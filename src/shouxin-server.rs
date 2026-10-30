//! ShouXin 服务器启动入口
//!
//! 架构说明 (v0.5.1):
//! - llm_runtime (native thread, spawned via handle.spawn()): 后台提取队列 + LLM 推理
//! - api_runtime (tokio multi-thread): HTTP API 请求处理
//! 两者完全隔离，HTTP handler 不受 LLM 推理阻塞影响（P-3 修复）
//! 优雅关闭: Ctrl+C 时执行 sled flush + close（P-1 修复）
//! 
//! TODO: 回归修复后，后台队列应使用 llm_runtime 运行（当前暂时退回 with_stores）

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use axum::serve;
use axum::middleware;
use axum::extract::{DefaultBodyLimit, State};
use axum::{Router, routing};
use axum::body::Body;
use clap::Parser;
use constant_time_eq::constant_time_eq;
use sqlx::sqlite::SqlitePoolOptions;
use tokio::net::TcpListener;

use shouxin::config::Settings;
use shouxin::heartbeat::{HeartbeatAnalyzer, HeartbeatExecutor, HeartbeatScheduler, QuestionPool, SchedulerConfig};
use shouxin::llm::factory::{build_llm_manager, build_heartbeat_client};
use shouxin::memory::any_vector_store::AnyVectorStore;
use shouxin::memory::ThreeLibraryMemoryConfig;
use shouxin::memory::ThreeLibraryMemoryManager;
use shouxin::memory_extractor::{LlmMemoryExtractor, DataDispatcher};
use shouxin::api::async_memory::AsyncMemory;
use shouxin::api::routes::create_router;
use shouxin::api::routes::FileApiState;
use shouxin::embedder::FastEmbedder;
use shouxin::stores::dictionary::{SledDictionaryStore, WriteIntentState};
use shouxin::stores::relational::RelationalStore;
use shouxin::stores::vector::factory::VectorStoreFactory;
use shouxin::stores::vector::hnsw_store::HnswVectorStore;
use shouxin::service::internal_api::{InternalApiState, create_internal_api_router};
use shouxin::traits::store_traits::DictionaryStoreTrait;
use shouxin::traits::{LlmClient, VectorStore};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 监听地址（如 127.0.0.1:3000）。不填则使用配置 [server] 的 host + port
    #[arg(long)]
    host: Option<String>,

    /// 数据根目录（SQLite / Sled / 向量库都放这里）。
    /// 不填则使用配置 [storage].data_dir；默认 "./data" 时保持历史布局（Sled 在 ./.sled_data）
    #[arg(long)]
    data_dir: Option<String>,
}

fn timeout_mw(timeout_secs: u64) -> impl Fn(axum::http::Request<Body>, axum::middleware::Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<axum::response::Response, axum::http::StatusCode>> + Send>> + Clone {
    move |request: axum::http::Request<Body>, next: axum::middleware::Next| {
        let fut = async move {
            match tokio::time::timeout(Duration::from_secs(timeout_secs), next.run(request)).await {
                Ok(response) => Ok(response),
                Err(_) => Err(axum::http::StatusCode::REQUEST_TIMEOUT),
            }
        };
        Box::pin(fut)
    }
}

/// Bearer Token 认证中间件，应用于 `/api/*` 路由。
///
/// - 仅对 `/api/` 前缀的路径生效；`/health` 等其他路径直接放行。
/// - 若 `SHOUXIN_API_TOKEN` 未设置（state 为 None），则跳过认证（开发模式便利）。
/// - 从 `Authorization: Bearer <token>` 头提取 token，使用常量时间比较。
/// - 认证失败返回 401 Unauthorized。
async fn api_auth_mw(
    State(expected_token): State<Option<Arc<String>>>,
    req: axum::http::Request<Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    // 仅对 /api/* 路径强制认证，/health 等其他路径直接放行
    if !req.uri().path().starts_with("/api/") {
        return next.run(req).await;
    }
    // 未配置 token 时跳过认证（开发模式便利）
    let expected = match &expected_token {
        Some(t) => t,
        None => return next.run(req).await,
    };
    // 从 Authorization: Bearer <token> 头提取 token
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() || !constant_time_eq(token.as_bytes(), expected.as_bytes()) {
        let body = axum::body::Body::from(
            serde_json::json!({"success": false, "error": "Missing or invalid API token"}).to_string(),
        );
        return axum::response::Response::builder()
            .status(axum::http::StatusCode::UNAUTHORIZED)
            .header("Content-Type", "application/json")
            .body(body)
            .unwrap();
    }
    next.run(req).await
}

/// API 限流状态 (S-04)
///
/// 使用内存计数器记录每 IP 在滑动窗口内的请求次数。
/// - `requests`: IP -> (窗口起始时间, 已请求次数)
/// - `max_requests`: 窗口内允许的最大请求数
/// - `window_secs`: 窗口大小（秒）
#[derive(Clone)]
struct RateLimiterState {
    requests: Arc<std::sync::Mutex<HashMap<String, (Instant, u32)>>>,
    max_requests: u32,
    window_secs: u64,
}

/// 限流中间件 (S-04)
///
/// 仅对以下 3 个 POST 端点生效：
/// - `POST /api/memory/add`
/// - `POST /api/memory/search/semantic`
/// - `POST /api/files/upload`
///
/// 客户端 IP 从 `X-Forwarded-For` 头获取（取第一个 IP），未设置时回退为 "unknown"。
/// 超过限流阈值时返回 `429 Too Many Requests`。
async fn rate_limit_mw(
    State(state): State<RateLimiterState>,
    req: axum::http::Request<Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let is_rate_limited = method == axum::http::Method::POST
        && (path == "/api/memory/add"
            || path == "/api/memory/search/semantic"
            || path == "/api/files/upload");

    if !is_rate_limited {
        return next.run(req).await;
    }

    // 从 X-Forwarded-For 头获取客户端 IP（取第一个），未设置时回退为 "unknown"
    let client_ip = req
        .headers()
        .get("X-Forwarded-For")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let now = Instant::now();
    let window = Duration::from_secs(state.window_secs);
    let max = state.max_requests;

    let allowed = {
        let mut requests = state.requests.lock().unwrap();
        match requests.get(&client_ip).cloned() {
            Some((window_start, count)) => {
                if now.duration_since(window_start) < window {
                    if count >= max {
                        false
                    } else {
                        requests.insert(client_ip.clone(), (window_start, count + 1));
                        true
                    }
                } else {
                    // 窗口已过期，重置计数
                    requests.insert(client_ip.clone(), (now, 1));
                    true
                }
            }
            None => {
                requests.insert(client_ip.clone(), (now, 1));
                true
            }
        }
    };

    if !allowed {
        tracing::warn!(
            "Rate limit exceeded for IP {} on {} (max {}/{})s",
            client_ip,
            path,
            max,
            state.window_secs
        );
        let body = axum::body::Body::from(
            serde_json::json!({
                "success": false,
                "error": format!(
                    "Rate limit exceeded. Maximum {} requests per {} seconds.",
                    max, state.window_secs
                )
            })
            .to_string(),
        );
        return axum::response::Response::builder()
            .status(axum::http::StatusCode::TOO_MANY_REQUESTS)
            .header("Content-Type", "application/json")
            .header("Retry-After", state.window_secs.to_string())
            .body(body)
            .unwrap();
    }

    next.run(req).await
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    std::panic::set_hook(Box::new(|info| {
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let payload = info.payload().downcast_ref::<&str>().copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("<unknown>");
        eprintln!("[PANIC] {} - {}", location, payload);
    }));

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    std::env::set_var("FASTEMBED_CACHE_DIR", std::env::current_dir()?.join(".fastembed_cache").display().to_string());
    std::env::set_var("HF_HUB_OFFLINE", "1");

    let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel(1);

    let args_clone = args.clone();
    let api_thread = thread::Builder::new()
        .name("api-server".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(std::thread::available_parallelism().map(|p| p.get()).unwrap_or(16))
                .max_blocking_threads(512)
                // 默认栈只有 2 MB。llama.cpp 的预分词用的是 MSVC 的 std::regex，
                // 它是**递归**匹配器，遇到长数字串/长无标点文本时递归深度接近字符数，
                // 2 MB 栈会直接爆掉：`regex_error(error_stack)`。
                // 这个 C++ 异常跨 FFI 抛回 Rust 无法被 catch_unwind 捕获，
                // 结果是 `fatal runtime error: Rust cannot catch foreign exceptions` —— 整个服务 abort。
                // 200 条压测实测：一条 1100 字符的纯数字串（F014）和一条含数学符号的条目
                // 各自触发一次，队列里 50+ 条任务全部中断。
                // 32 MB 足够覆盖单次推理里的分词递归（远小于常见的 8 GB 内存占用）。
                .thread_stack_size(32 * 1024 * 1024)
                .enable_all()
                .build()
                .expect("Failed to create api_runtime");
            rt.block_on(async move {
                if let Err(e) = run_api_server(args_clone, shutdown_tx).await {
                    eprintln!("API server error: {}", e);
                }
            });
        })?;

    api_thread.join().ok();
    eprintln!("Server process exited");
    Ok(())
}

async fn init_builtin_virtual_characters(relational: &RelationalStore) -> Result<(), anyhow::Error> {
    use shouxin::stores::relational::{BuiltInVirtualCharacters, NewVirtualCharacter};

    let store = relational.virtual_alias_store();
    let existing = store.list(None, true, 5000).await?;
    let existing_names: std::collections::HashSet<String> = existing
        .iter()
        .map(|v| v.character.name.to_lowercase())
        .collect();

    let mut inserted_count = 0;

    for (name, _id, aliases) in BuiltInVirtualCharacters::get_all() {
        if existing_names.contains(&name.to_lowercase()) {
            continue;
        }

        // 内置角色统一放在 system 名下，避免和用户自建角色混在一起
        let alias_list: Vec<shouxin::stores::relational::VirtualCharacterAlias> = aliases
            .iter()
            .map(|(alias_name, source_work, alias_type)| {
                shouxin::stores::relational::VirtualCharacterAlias::new(
                    String::new(),
                    alias_name.to_string(),
                    Some(source_work.to_string()),
                    Some(alias_type.to_string()),
                )
            })
            .collect();

        let source_work = aliases
            .first()
            .map(|(_, work, _)| work.to_string());

        let input = NewVirtualCharacter {
            name: name.to_string(),
            source_work,
            canon: None,
            persona: None,
            category: Some("builtin".to_string()),
            owner_id: Some("system".to_string()),
            is_builtin: true,
            aliases: alias_list,
        };

        match store.create(input).await {
            Ok(_) => {
                inserted_count += 1;
                println!("Initialized built-in virtual character: {}", name);
            }
            Err(e) => {
                eprintln!("Failed to initialize built-in virtual character {}: {}", name, e);
            }
        }
    }

    println!(
        "Built-in virtual characters initialized: {} inserted, {} already existed",
        inserted_count,
        BuiltInVirtualCharacters::get_all().len() - inserted_count
    );

    Ok(())
}

async fn run_api_server(
    args: Args,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> anyhow::Result<()> {
    let cache_dir = std::env::current_dir()?.join(".fastembed_cache");
    std::fs::create_dir_all(&cache_dir)?;
    std::env::set_var("FASTEMBED_CACHE_DIR", cache_dir.display().to_string());
    println!("FastEmbed cache directory: {}", cache_dir.display());

    println!("Starting ShouXin Server...");

    let store_type = std::env::var("SHOUXIN_STORE_TYPE").unwrap_or_else(|_| "embedded".to_string());
    println!("Vector store type: {} (from SHOUXIN_STORE_TYPE env)", store_type);

    // 先加载配置：数据路径、监听地址等都要用
    let settings = match Settings::new() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to load config: {}, using defaults", e);
            Settings::default()
        }
    };
    tracing::info!("Loaded config: LLM primary={}, secondary={:?}", settings.llm.primary, settings.llm.secondary);

    // ------------------------------------------------------------------
    // 数据目录解析：命令行 --data-dir > 配置 [storage].data_dir > 默认 "./data"
    //
    // 兼容性：默认值 "./data" 保持历史布局（Sled 在仓库根的 ./.sled_data），
    // 避免老用户升级后"数据像丢了"；一旦显式指定了别的目录，
    // 所有数据（SQLite / Sled / 向量库）统一收敛到该目录下，便于多实例隔离与备份。
    // ------------------------------------------------------------------
    let data_root = args
        .data_dir
        .clone()
        .unwrap_or_else(|| settings.storage.data_dir.clone());
    let is_legacy_layout = data_root == "./data" || data_root == "data" || data_root.is_empty();

    let (db_path, sled_path, vector_base) = if is_legacy_layout {
        let dir = if store_type == "hnsw" { "./data_hnsw" } else { "./data" };
        let sled = if store_type == "hnsw" { "./sled_data_hnsw".to_string() } else { ".sled_data".to_string() };
        let vec = if store_type == "hnsw" {
            std::path::PathBuf::from("./data_hnsw").join("vector")
        } else {
            std::path::PathBuf::from(dir).join(&settings.storage.vector_dir)
        };
        (std::env::current_dir()?.join(dir).join("shouxin.db"), sled, vec)
    } else {
        let base = std::path::PathBuf::from(&data_root);
        let sled = base.join(&settings.storage.dictionary_dir).to_string_lossy().to_string();
        let vec = base.join(&settings.storage.vector_dir);
        (base.join("shouxin.db"), sled, vec)
    };
    println!("Data root: {}（SQLite: {}）", data_root, db_path.display());
    std::fs::create_dir_all(db_path.parent().unwrap())?;
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());

    let pool = SqlitePoolOptions::new()
        .max_connections(64)
        .connect(&db_url)
        .await?;

    sqlx::query("PRAGMA encoding = 'UTF-8'").execute(&pool).await?;
    sqlx::query("PRAGMA journal_mode=WAL").execute(&pool).await?;
    sqlx::query("PRAGMA busy_timeout=5000").execute(&pool).await?;
    sqlx::query("PRAGMA synchronous=NORMAL").execute(&pool).await?;
    let pool_for_shutdown = pool.clone();
    let relational = RelationalStore::new(pool);
    relational.init_schema().await?;
    init_builtin_virtual_characters(&relational).await?;
    let relational = Arc::new(relational);
    println!("Relational store initialized (file: {})", db_path.display());

    let dictionary = SledDictionaryStore::open_with_paging(
        &sled_path,
        shouxin::stores::dictionary::DayPagingConfig {
            cutoff_hour: settings.storage.day_cutoff_hour,
            utc_offset_minutes: settings.storage.utc_offset_minutes,
        },
    )?;
    // 上次进程若在处理中途退出，对话状态会卡在 processing；先收回成 failed 再恢复队列
    match dictionary.reset_stale_processing() {
        Ok(n) if n > 0 => println!("Recovered {} conversation(s) stuck in `processing`", n),
        Ok(_) => {}
        Err(e) => eprintln!("reset_stale_processing failed: {}", e),
    }
    let dictionary = Arc::new(dictionary);
    {
        let (cutoff, offset) = dictionary.day_paging();
        println!(
            "Dictionary store initialized (按天分页: 每天 {}:00 换页, UTC{:+} 分钟)",
            cutoff, offset
        );
    }

    // 监听地址：命令行 --host 优先，其次 config.toml 的 [server] host+port
    let bind_addr = args
        .host
        .clone()
        .unwrap_or_else(|| format!("{}:{}", settings.server.host, settings.server.port));

    // 主智能体 ID 是全系统 owner_id 的唯一来源，必须在任何路由/存储初始化之前注入
    shouxin::core::owner::init_primary_agent_id(settings.agent.primary_agent_id.clone());

    // 事件合并阈值（与嵌入模型相关，必须可配置；见 config.toml [extraction]）
    shouxin::service::background_queue::set_merge_thresholds(
        settings.extraction.event_merge_auto_threshold,
        settings.extraction.event_merge_pending_threshold,
    );

    // 向量库路径由上面的数据目录解析统一给出
    let vector_path = vector_base.to_string_lossy().to_string();
    let vector_config = shouxin::stores::vector::factory::VectorStoreConfig {
        path: vector_path.clone(),
        ..shouxin::stores::vector::factory::VectorStoreConfig::default()
    };

    let vector_store_arc: Arc<AnyVectorStore> = if store_type == "hnsw" {
        let store = HnswVectorStore::new_with_dimension(&vector_path, vector_config.dimension)?;
        let store = Arc::new(store);
        let loaded = store.load_existing().await?;
        tracing::info!("HNSW vector store created, loaded {} existing records", loaded);
        println!("HNSW vector store created, loaded {} existing records", loaded);
        Arc::new(AnyVectorStore::Hnsw(store))
    } else {
        let vector_concrete = VectorStoreFactory::create_embedded_concrete(&vector_config)?;
        println!("Embedded vector store created");
        Arc::new(AnyVectorStore::Embedded(vector_concrete))
    };
    let vector: Arc<dyn VectorStore> = vector_store_arc.clone();
    println!("Vector store initialized (path: {})", vector_path);

    let llm_manager = build_llm_manager(&settings.llm).await?;
    let llm_manager = Arc::new(llm_manager);
    let llm_client: Arc<dyn LlmClient> = llm_manager.clone();
    println!("LLM client manager initialized (primary: {})", settings.llm.primary);

    match llm_client.preload().await {
        Ok(()) => tracing::info!("LLM model preload completed"),
        Err(e) => tracing::warn!("LLM model preload returned error (may still work on first request): {}", e),
    }

    let llm_client_for_heartbeat: Arc<dyn LlmClient> = if let Some(ref hb_settings) = settings.llm.heartbeat {
        match build_heartbeat_client(hb_settings) {
            Ok(client) => {
                println!("Heartbeat LLM client initialized (backend: {}, model: {})", hb_settings.backend, hb_settings.model);
                Arc::from(client)
            }
            Err(e) => {
                tracing::warn!("Failed to create heartbeat LLM client: {}; falling back to main LLM client", e);
                llm_client.clone()
            }
        }
    } else {
        tracing::info!("No [llm.heartbeat] config, heartbeat will share main LLM client");
        llm_client.clone()
    };

    // 嵌入器：按配置选择 fastembed / hash，缺模型时不再阻塞启动
    let embedder = shouxin::embedder::build_embedder(&settings.embedder)?;

    let extractor = LlmMemoryExtractor::new(llm_client.clone());
    println!("Memory extractor initialized (LlmMemoryExtractor, backend: {})", settings.llm.primary);

    let data_dispatcher = DataDispatcher::new(
        dictionary.clone(),
        relational.clone(),
        vector.clone(),
    )
    .with_embedder(embedder.clone());
    let data_dispatcher = Arc::new(data_dispatcher);
    println!("Data dispatcher initialized");

    let manager_config = ThreeLibraryMemoryConfig::new()
        .with_user_id("default_user".to_string())
        .with_agent_id(settings.agent.primary_agent_id.clone())
        .with_event_settings(settings.event.clone())
        .with_extraction_settings(settings.extraction.clone());
    println!("Agent config: primary_agent_id={}", settings.agent.primary_agent_id);

    let relational_for_internal = relational.clone();
    let dict_clone = dictionary.clone();
    let vs_shutdown = vector_store_arc.clone();
    let vs_file = vector_store_arc.clone();
    let emb_clone = embedder.clone();
    let llm_for_queue = llm_client.clone();

    let mut manager = ThreeLibraryMemoryManager::with_embedded_vector_store(
        dictionary.clone(),
        relational.clone(),
        vector.clone(),
        vector_store_arc.clone(),
        llm_client.clone(),
        embedder.clone(),
        Arc::new(extractor),
        data_dispatcher.clone(),
    );
    manager.set_config(manager_config);
    println!("Three library memory manager initialized");

    let extractor_for_queue = LlmMemoryExtractor::new(llm_for_queue);

    // TODO: 回归修复后，后台队列应使用 llm_runtime 运行
    let background_queue = shouxin::service::background_queue::BackgroundTaskQueue::with_stores(
        dict_clone.clone(),
        relational_for_internal.clone(),
        vs_file.clone() as Arc<dyn VectorStore>,
        Arc::new(extractor_for_queue),
        emb_clone.clone(),
        "default_user".to_string(),
        settings.extraction.timeout_secs,
    );
    background_queue.recover_pending_tasks();

    // 跨库写入意图恢复：崩溃会留下 `prepared` 意图，意味着派生库可能只写了一半。
    // 这里把陈旧意图标记为 aborted，并把对应对话置为 failed（等待重放）。
    // 补偿删除（按 intent 记录的事件/向量 ID 回滚）在下一阶段实现。
    match dict_clone.list_write_intents(Some(WriteIntentState::Prepared)) {
        Ok(intents) => {
            let grace =
                chrono::Duration::seconds(settings.extraction.intent_grace_secs as i64);
            let now = chrono::Utc::now();
            let mut recovered = 0usize;

            for intent in intents {
                if now - intent.started_at < grace {
                    continue;
                }
                if let Err(e) = dict_clone.finish_write_intent(
                    &intent.conversation_id,
                    WriteIntentState::Aborted,
                    Some("interrupted write: process restart"),
                ) {
                    tracing::warn!(
                        "failed to abort stale write intent {}: {}",
                        intent.conversation_id,
                        e
                    );
                    continue;
                }
                if let Err(e) = dict_clone
                    .update_conversation_status(
                        &intent.conversation_id,
                        "failed",
                        Some("interrupted write: partial derived data, needs replay"),
                    )
                    .await
                {
                    tracing::warn!(
                        "failed to mark conversation {} as failed: {}",
                        intent.conversation_id,
                        e
                    );
                }

                // 补偿：按意图里登记的 ID 回滚残留的派生数据（关系库 → 向量库）
                for event_id in &intent.created_event_ids {
                    match relational_for_internal.delete_event_by_id(event_id).await {
                        Ok(_) => tracing::info!("recovery: deleted residual event {}", event_id),
                        Err(e) => tracing::warn!(
                            "recovery: failed to delete residual event {}: {}",
                            event_id,
                            e
                        ),
                    }
                }
                if !intent.created_memory_ids.is_empty() {
                    match vector_store_arc
                        .delete_batch(intent.created_memory_ids.clone())
                        .await
                    {
                        Ok(()) => tracing::info!(
                            "recovery: deleted {} residual vector memories",
                            intent.created_memory_ids.len()
                        ),
                        Err(e) => tracing::warn!(
                            "recovery: failed to delete residual vector memories: {}",
                            e
                        ),
                    }
                }

                recovered += 1;
            }

            if recovered > 0 {
                println!(
                    "Recovered {} stale write intents (marked aborted, conversations marked failed)",
                    recovered
                );
            }
        }
        Err(e) => tracing::warn!("failed to scan write intents: {}", e),
    }

    let background_queue = Arc::new(background_queue);
    manager.set_background_queue(background_queue.clone());
    println!("Background extraction queue initialized");

    let async_memory = AsyncMemory::new(Arc::new(manager))
        .with_todo_settings(settings.todo.clone());
    let async_memory = Arc::new(async_memory);
    println!("Async memory API initialized");

    let dict_for_heartbeat = dict_clone.clone();
    let rel_for_heartbeat = relational_for_internal.clone();

    // 问题池目录来自配置（原实现写死 ".heartbeat_data"，配置项形同虚设）
    let question_pool_dir = format!(
        "{}/{}",
        data_root, settings.storage.question_pool_dir
    );
    let question_pool = Arc::new(QuestionPool::new(&question_pool_dir, rel_for_heartbeat.clone()).map_err(|e| anyhow::anyhow!("Failed to create question pool at {}: {}", question_pool_dir, e))?);
    let analyzer = Arc::new(
        HeartbeatAnalyzer::new(
            llm_client_for_heartbeat,
            dict_for_heartbeat,
            rel_for_heartbeat.clone(),
        )
        // 画像频率限制来自 [heartbeat]，默认 2 次/天、间隔 8 小时；
        // 批量导入场景（几百条语料）需要调大，否则当天大量联系人排不上队。
        .with_profile_limits(
            settings.heartbeat.profile_max_updates_per_day,
            settings.heartbeat.profile_update_interval_hours,
        )
        // CPU 推理下画像 JSON 需要数分钟，旧硬编码 90s 会让画像永远超时
        .with_profile_timeout_secs(settings.heartbeat.profile_timeout_secs),
    );
    let executor = Arc::new(HeartbeatExecutor::new(
        analyzer,
        question_pool.clone(),
        rel_for_heartbeat,
    )
    .with_vector_store(vector_store_arc.clone())
    .with_todo_settings(settings.todo.clone()));
    // 全部来自 [heartbeat] 配置（enabled/adaptive/超时/阈值），不在代码里写死
    let scheduler_config = SchedulerConfig::from_settings(&settings.heartbeat);
    let scheduler_config_clone = scheduler_config.clone();
    let heartbeat_scheduler = Arc::new(HeartbeatScheduler::new(
        executor,
        question_pool,
        scheduler_config,
    ));
    let _heartbeat_handle = heartbeat_scheduler.clone().spawn();
    if scheduler_config_clone.enabled {
        println!(
            "Heartbeat scheduler started (base: {}s, adaptive: {}, question_pool: {})",
            scheduler_config_clone.base_interval_secs,
            scheduler_config_clone.adaptive,
            question_pool_dir
        );
    } else {
        println!("Heartbeat scheduler disabled by config ([heartbeat].enabled = false)");
    }

    let file_state = FileApiState {
        file_store: relational_for_internal.file_store().clone(),
        dictionary: dict_clone.clone(),
        vector_store: vs_file as Arc<dyn VectorStore>,
        embedder: emb_clone.clone(),
        data_dir: data_root.clone(),
    };
    let file_state = Arc::new(file_state);

    let app = create_router(async_memory.clone(), file_state, heartbeat_scheduler.clone());

    // 公共 API Bearer Token 认证中间件（仅 /api/* 路由，/health 免认证）
    let api_token: Option<Arc<String>> = std::env::var("SHOUXIN_API_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .map(Arc::new);
    if api_token.is_some() {
        tracing::info!("API Bearer token authentication enabled for /api/* routes");
    } else {
        tracing::info!("API Bearer token authentication disabled (SHOUXIN_API_TOKEN not set)");
    }
    let app = app.layer(middleware::from_fn_with_state(api_token, api_auth_mw));

    // S-04: API 限流中间件（仅对 POST /api/memory/add, /api/memory/search/semantic, /api/files/upload 生效）
    // 默认每 IP 每分钟 60 次，可通过 SHOUXIN_RATE_LIMIT_PER_MINUTE 环境变量调整
    let rate_limit_per_minute: u32 = std::env::var("SHOUXIN_RATE_LIMIT_PER_MINUTE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let rate_limiter_state = RateLimiterState {
        requests: Arc::new(std::sync::Mutex::new(HashMap::new())),
        max_requests: rate_limit_per_minute,
        window_secs: 60,
    };
    tracing::info!(
        "Rate limiting enabled for POST /api/memory/add, /api/memory/search/semantic, /api/files/upload ({} req/min per IP)",
        rate_limit_per_minute
    );
    let app = app.layer(middleware::from_fn_with_state(rate_limiter_state, rate_limit_mw));

    let app = app.layer(DefaultBodyLimit::max(2 * 1024 * 1024));

    let internal_api_state = InternalApiState {
        relational_store: relational_for_internal.clone(),
        primary_agent_id: settings.agent.primary_agent_id.clone(),
    };
    let internal_router = create_internal_api_router(internal_api_state);
    let app = app.nest("/internal", internal_router);
    println!("Internal API routes mounted at /internal");

    // S-06: 统一认证方案 - 管理 API 优先从 INTERNAL_API_TOKEN 环境变量读取 token（与内部 API 一致），
    // 若环境变量未设置则回退到 config.toml 的 internal_api_token 配置
    let admin_token = std::env::var("INTERNAL_API_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| settings.agent.internal_api_token.clone());
    if admin_token.is_empty() {
        tracing::warn!("Admin API token is empty; admin endpoints will reject all requests");
    } else {
        tracing::info!("Admin API token configured via INTERNAL_API_TOKEN env or config (value hidden)");
    }

    let admin_token_for_mw = Arc::new(admin_token.clone());
    let admin_llm_state = llm_manager.clone();
    let admin_router = Router::new()
        .route("/admin/llm/status", routing::get({
            let state = admin_llm_state.clone();
            move || async move {
                let status = state.get_status().await;
                axum::Json(serde_json::json!({
                    "success": true,
                    "data": {
                        "active_source": status.active_source,
                        "is_primary": status.is_primary,
                        "total_requests": status.total_requests,
                        "total_failures": status.total_failures,
                        "primary_healthy": status.primary_healthy,
                        "secondary_healthy": status.secondary_healthy,
                        "auto_switch_enabled": status.auto_switch_enabled,
                    }
                }))
            }
        }))
        .route("/admin/llm/switch", routing::post({
            let state = admin_llm_state.clone();
            move |axum::Json(body): axum::Json<serde_json::Value>| async move {
                let target = body.get("target").and_then(|v| v.as_str()).unwrap_or("primary");
                match state.switch_to(target).await {
                    Ok(current) => axum::Json(serde_json::json!({"success": true, "data": {"switched_to": current}})),
                    Err(e) => axum::Json(serde_json::json!({"success": false, "error": e.to_string()})),
                }
            }
        }));
    let admin_router = admin_router.layer(middleware::from_fn({
        let token = admin_token_for_mw.clone();
        let token_clone = token.clone();
        move |req: axum::http::Request<Body>, next: axum::middleware::Next| {
            let token = token_clone.clone();
            async move {
                // S-06: 统一使用 Authorization: Bearer <token> 头（与内部 API 和公共 API 一致）
                let auth_header = req.headers().get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
                let header_token = auth_header.strip_prefix("Bearer ").unwrap_or("");
                if header_token.is_empty() || !constant_time_eq(header_token.as_bytes(), token.as_bytes()) {
                    let body = axum::body::Body::from(
                        serde_json::json!({"success": false, "error": "Missing or invalid Authorization Bearer token"}).to_string()
                    );
                    return axum::response::Response::builder()
                        .status(axum::http::StatusCode::UNAUTHORIZED)
                        .header("Content-Type", "application/json")
                        .body(body).unwrap();
                }
                next.run(req).await
            }
        }
    }));
    let app = app.nest("/", admin_router);
    println!("Admin API routes mounted at /admin/llm (auth required)");

    let http_timeout = settings.llm.timeout_secs.max(30);
    let app = app.layer(middleware::from_fn(timeout_mw(http_timeout)));
    println!("HTTP request timeout set to {}s", http_timeout);

    println!("Server starting on {}", bind_addr);
    let listener = TcpListener::bind(&bind_addr).await?;
    println!("Server listening on {}", bind_addr);

    let warmup_am = async_memory.clone();
    tokio::spawn(async move {
        tracing::info!("Semantic search warmup started...");
        let scope = shouxin::core::contact::Scope::User;
        match warmup_am.semantic_search("warmup", 1, scope, Some("__warmup__"), None).await {
            Ok(results) => tracing::info!("Semantic search warmup complete ({} results)", results.len()),
            Err(e) => tracing::warn!("Semantic search warmup returned error (non-critical): {}", e),
        }
    });

    let vs_for_shutdown = vs_shutdown.clone();
    let dict_for_shutdown = dict_clone.clone();
    let mut shutdown_rx = shutdown_tx.subscribe();

    let shutdown_future = async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nShutdown signal received (Ctrl+C) - initiating graceful shutdown...");
            }
            _ = shutdown_rx.recv() => {
                eprintln!("External shutdown signal received - initiating graceful shutdown...");
            }
        }

        tracing::info!("Initiating graceful shutdown...");

        tracing::info!("Waiting for vector store to persist...");
        match vs_for_shutdown.persist().await {
            Ok(count) => {
                tracing::info!("Vector store persisted ({} records)", count);
                eprintln!("Vector store persisted ({} records)", count);
            }
            Err(e) => {
                tracing::error!("Failed to persist vector store: {}", e);
                eprintln!("ERROR: Failed to persist vector store: {}", e);
            }
        }

        tracing::info!("Running SQLite WAL checkpoint...");
        match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&pool_for_shutdown).await {
            Ok(_) => tracing::info!("SQLite WAL checkpoint completed"),
            Err(e) => tracing::error!("SQLite WAL checkpoint failed: {}", e),
        }

        tracing::info!("Flushing and closing sled dictionary store (P-1 fix: no lock residue)...");
        if let Err(e) = dict_for_shutdown.flush() {
            tracing::error!("Failed to flush sled dictionary: {}", e);
            eprintln!("ERROR: Failed to flush sled dictionary: {}", e);
        }
        if let Err(e) = dict_for_shutdown.close() {
            tracing::error!("Failed to close sled dictionary: {}", e);
            eprintln!("ERROR: Failed to close sled dictionary: {}", e);
        } else {
            tracing::info!("Sled dictionary closed gracefully (no lock residue)");
            eprintln!("Sled dictionary closed gracefully (no lock residue)");
        }

        tracing::info!("Graceful shutdown complete");
        eprintln!("Server gracefully shut down");
    };

    let server = serve(listener, app);

    tokio::select! {
        result = server => {
            if let Err(e) = result {
                eprintln!("Server error: {}", e);
            }
        }
        _ = shutdown_future => {
            tracing::info!("Shutdown sequence completed");
        }
    }

    Ok(())
}
