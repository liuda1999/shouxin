//! Embedding模块
//!
//! 提供本地嵌入能力：
//! - `fastembed`：ONNX 模型（默认，需要模型文件或网络）
//! - `hash`：离线哈希嵌入（无模型依赖，供开发/自测/降级使用）

pub mod factory;
pub mod fastembed;
pub mod hash;
pub mod mock;

use std::sync::Arc;

use crate::config::settings::EmbedderSettings;
use crate::error::ShouxinError;
use crate::traits::embedder::Embedder;

pub use self::factory::{EmbedderFactory, EmbedderType};
pub use self::fastembed::FastEmbedder;
pub use self::hash::HashEmbedder;
pub use self::mock::MockEmbedder;

/// 根据配置构建嵌入器
///
/// 选择逻辑：
/// 1. `backend = "hash"` → 直接用离线哈希嵌入（不触网、不需要模型）；
/// 2. `backend = "fastembed"` → 加载 ONNX 模型；
///    失败且 `fallback_to_hash = true` 时降级为哈希嵌入并打 WARN，否则报错退出。
///
/// 这样"缺少模型文件"不会再阻塞服务启动。
pub fn build_embedder(settings: &EmbedderSettings) -> Result<Arc<dyn Embedder>, ShouxinError> {
    let backend = settings.backend.to_ascii_lowercase();

    if backend == "hash" {
        let embedder = HashEmbedder::new(settings.hash_dimension, "hash-ngram");
        println!(
            "Embedder initialized (backend: hash, dimension: {})",
            settings.hash_dimension
        );
        return Ok(Arc::new(embedder));
    }

    // 缓存目录：配置里的 model_path 优先（未显式设置环境变量时）
    if std::env::var("FASTEMBED_CACHE_DIR").is_err() && !settings.model_path.is_empty() {
        std::env::set_var("FASTEMBED_CACHE_DIR", &settings.model_path);
    }

    match FastEmbedder::new(&settings.model) {
        Ok(e) => {
            println!(
                "Embedder initialized (backend: fastembed, model: {})",
                settings.model
            );
            Ok(Arc::new(e))
        }
        Err(e) if settings.fallback_to_hash => {
            tracing::warn!(
                "FastEmbed 初始化失败（{}），已按 fallback_to_hash 降级为离线哈希嵌入。\
                 语义检索质量会下降，生产环境请配置可用模型。",
                e
            );
            println!("Embedder fallback to hash (fastembed unavailable: {})", e);
            Ok(Arc::new(HashEmbedder::new(
                settings.hash_dimension,
                "hash-ngram-fallback",
            )))
        }
        Err(e) => Err(e),
    }
}
