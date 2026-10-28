//! 全局配置系统
//!
//! 支持从 config.toml 文件加载配置，并可通过环境变量覆盖
//! 环境变量格式: SHOUXIN_<SECTION>__<KEY> (双下划线分隔嵌套)

use config::{Config, Environment};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::llm::LlmConfig;
use crate::config::embedder::EmbedderConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub llm: LlmSettings,
    pub embedder: EmbedderSettings,
    pub heartbeat: HeartbeatSettings,
    pub storage: StorageSettings,
    pub logging: LoggingSettings,
    pub agent: AgentSettings,
    #[serde(default)]
    pub event: EventSettings,
    #[serde(default)]
    pub extraction: ExtractionSettings,
    #[serde(default)]
    pub todo: TodoSettings,
}

/// 待办生命周期策略
///
/// 收敛原先散落在心跳执行器与 API 层的 30/50 硬编码阈值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoSettings {
    /// 待办数量超过该值时，归档最旧的一批
    #[serde(default = "default_todo_max_pending_before_archive")]
    pub max_pending_before_archive: usize,
    /// 归档时保留的待办数量
    #[serde(default = "default_todo_archive_keep_count")]
    pub archive_keep_count: usize,
    /// 待办数量超过该值时，由心跳生成"任务提醒"
    #[serde(default = "default_todo_reminder_threshold")]
    pub reminder_threshold: usize,
    /// 超过该天数未更新视为陈旧，可被归档
    #[serde(default = "default_todo_archive_after_days")]
    pub archive_after_days: i64,
}

fn default_todo_max_pending_before_archive() -> usize { 50 }
fn default_todo_archive_keep_count() -> usize { 50 }
fn default_todo_reminder_threshold() -> usize { 30 }
fn default_todo_archive_after_days() -> i64 { 30 }

impl Default for TodoSettings {
    fn default() -> Self {
        Self {
            max_pending_before_archive: default_todo_max_pending_before_archive(),
            archive_keep_count: default_todo_archive_keep_count(),
            reminder_threshold: default_todo_reminder_threshold(),
            archive_after_days: default_todo_archive_after_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSettings {
    pub long_term_hot_limit: usize,
    pub short_term_hot_limit: usize,
}

impl Default for EventSettings {
    fn default() -> Self {
        Self {
            long_term_hot_limit: 200,
            short_term_hot_limit: 100,
        }
    }
}

/// 记忆提取流水线配置
///
/// 用于替代代码中散落的硬编码阈值，后续可在此扩展重试策略、并发度等参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionSettings {
    /// 单次 LLM 提取超时（秒）
    #[serde(default = "default_extraction_timeout_secs")]
    pub timeout_secs: u64,
    /// 启动时"多旧的 prepared 写入意图"视为崩溃残留（秒）
    #[serde(default = "default_extraction_intent_grace_secs")]
    pub intent_grace_secs: u64,
}

fn default_extraction_timeout_secs() -> u64 { 600 }

fn default_extraction_intent_grace_secs() -> u64 { 300 }

impl Default for ExtractionSettings {
    fn default() -> Self {
        Self {
            timeout_secs: default_extraction_timeout_secs(),
            intent_grace_secs: default_extraction_intent_grace_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    pub primary_agent_id: String,
    pub internal_api_token: String,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            primary_agent_id: "primary_agent_default".to_string(),
            internal_api_token: String::new(),
        }
    }
}

/// LLM 多源配置（v0.4.0）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettings {
    /// 主模型来源: "ollama", "gguf", "api"
    pub primary: String,
    /// 备模型来源（可选）
    #[serde(default)]
    pub secondary: Option<String>,
    /// 全局温度
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    /// 全局最大令牌数
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// 全局超时（秒）
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Ollama 后端配置
    #[serde(default)]
    pub ollama: Option<OllamaBackendSettings>,
    /// GGUF 后端配置
    #[serde(default)]
    pub gguf: Option<GgufBackendSettings>,
    /// API 后端配置
    #[serde(default)]
    pub api: Option<ApiBackendSettings>,
    /// 心跳模块独立 LLM 配置（可选）
    #[serde(default)]
    pub heartbeat: Option<HeartbeatLlmSettings>,
}

fn default_temperature() -> f32 { 0.7 }
fn default_max_tokens() -> u32 { 2000 }
fn default_timeout_secs() -> u64 { 60 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaBackendSettings {
    pub model: String,
    pub endpoint: String,
    #[serde(default = "default_ollama_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_ollama_retries")]
    pub max_retries: u32,
}

fn default_ollama_timeout() -> u64 { 120 }
fn default_ollama_retries() -> u32 { 2 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GgufBackendSettings {
    pub model_path: String,
    #[serde(default = "default_context_size")]
    pub context_size: usize,
    #[serde(default = "default_threads")]
    pub threads: usize,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default)]
    pub n_gpu_layers: usize,
    /// 是否使用 GPU 推理
    ///
    /// false = 纯 CPU（模型加载到内存，不做 GPU 卸载）；
    /// true 且 n_gpu_layers=0 时按显存自动决定卸载层数。
    #[serde(default = "default_gguf_use_gpu")]
    pub use_gpu: bool,

    /// GPU 推理时允许占用的 CPU 核心上限（百分比）
    ///
    /// 即使 GPU 承担了主要计算，llama.cpp 仍会开多个线程做数据搬运/采样等，
    /// 线程数取 min(threads, 核心数 × 该百分比)。默认 60。
    #[serde(default = "default_gpu_max_cpu_percent")]
    pub gpu_max_cpu_percent: u32,
}

/// GPU 开关默认值：**false**
///
/// 原则：本项目是后台服务，显存属于"重要且稀缺"的资源，默认一律纯 CPU 推理
/// （模型驻留内存）。需要 GPU 时必须显式打开 `use_gpu = true`。
fn default_gguf_use_gpu() -> bool {
    false
}

/// GPU 推理时允许占用的 CPU 核心上限（百分比），默认 60%
///
/// 后台服务不能把核心吃满，否则会拖慢同机的其它进程。
fn default_gpu_max_cpu_percent() -> u32 {
    60
}

fn default_context_size() -> usize { 2048 }
fn default_threads() -> usize { 4 }
fn default_batch_size() -> usize { 512 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiBackendSettings {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_api_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_api_retries")]
    pub max_retries: u32,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

fn default_api_timeout() -> u64 { 30 }
fn default_api_retries() -> u32 { 3 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatLlmSettings {
    pub backend: String,
    pub model: String,
    pub endpoint: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedderSettings {
    /// 模型缓存目录（HF/fastembed 缓存根）
    pub model_path: String,
    /// 模型仓库 ID（HF 形式，如 "intfloat/multilingual-e5-base"）
    ///
    /// 与 `model_path` 区分：前者是模型标识，后者是本地缓存位置。
    #[serde(default = "default_embedder_model")]
    pub model: String,
    /// 嵌入后端：`fastembed`（ONNX 模型）或 `hash`（离线哈希，无模型依赖）
    #[serde(default = "default_embedder_backend")]
    pub backend: String,
    /// 离线哈希后端的维度（须与向量库维度一致，默认 768）
    #[serde(default = "default_hash_dimension")]
    pub hash_dimension: usize,
    /// FastEmbed 初始化失败时是否自动降级为哈希嵌入
    #[serde(default)]
    pub fallback_to_hash: bool,
}

fn default_embedder_backend() -> String {
    "fastembed".to_string()
}

fn default_embedder_model() -> String {
    "intfloat/multilingual-e5-base".to_string()
}

fn default_hash_dimension() -> usize {
    768
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatSettings {
    pub enabled: bool,
    pub base_interval_secs: u64,
    pub min_interval_secs: u64,
    pub max_interval_secs: u64,
    pub adaptive: bool,
    pub idle_threshold_secs: u64,
    /// 维护阶段超时（秒）——纯 SQL，不应调用 LLM
    #[serde(default = "default_heartbeat_maintenance_timeout_secs")]
    pub maintenance_timeout_secs: u64,
    /// LLM 阶段超时（秒）
    #[serde(default = "default_heartbeat_llm_timeout_secs")]
    pub llm_timeout_secs: u64,
    /// 可用内存低于该值时跳过心跳（MB）
    #[serde(default = "default_heartbeat_memory_threshold_mb")]
    pub memory_threshold_mb: u64,
    /// 低内存时最多连续跳过的次数（超过则强制执行）
    #[serde(default = "default_heartbeat_max_skips")]
    pub max_consecutive_skips: u32,
}

fn default_heartbeat_memory_threshold_mb() -> u64 {
    4096
}

fn default_heartbeat_max_skips() -> u32 {
    3
}

fn default_heartbeat_maintenance_timeout_secs() -> u64 {
    30
}

fn default_heartbeat_llm_timeout_secs() -> u64 {
    120
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSettings {
    pub data_dir: String,
    pub dictionary_dir: String,
    pub relational_dir: String,
    pub vector_dir: String,
    pub relational_db_file: String,
    pub vector_index_file: String,
    pub question_pool_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettings {
    pub level: String,
    pub dir: String,
    pub max_file_size_mb: u64,
}

#[allow(clippy::derivable_impls)]
impl Default for Settings {
    fn default() -> Self {
        Self {
            llm: LlmSettings::default(),
            embedder: EmbedderSettings::default(),
            heartbeat: HeartbeatSettings::default(),
            storage: StorageSettings::default(),
            logging: LoggingSettings::default(),
            agent: AgentSettings::default(),
            event: EventSettings::default(),
            extraction: ExtractionSettings::default(),
            todo: TodoSettings::default(),
        }
    }
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            primary: "ollama".to_string(),
            secondary: None,
            temperature: 0.7,
            max_tokens: 2000,
            timeout_secs: 60,
            ollama: Some(OllamaBackendSettings::default()),
            gguf: None,
            api: None,
            heartbeat: Some(HeartbeatLlmSettings::default()),
        }
    }
}

impl Default for OllamaBackendSettings {
    fn default() -> Self {
        Self {
            model: "llama3.2:3b".to_string(),
            endpoint: "http://localhost:11434".to_string(),
            timeout_secs: 120,
            max_retries: 2,
        }
    }
}

impl Default for HeartbeatLlmSettings {
    fn default() -> Self {
        Self {
            backend: "ollama".to_string(),
            model: "qwen2.5:1.5b".to_string(),
            endpoint: "http://localhost:11434".to_string(),
            temperature: 0.3,
            max_tokens: 500,
        }
    }
}

impl Default for EmbedderSettings {
    fn default() -> Self {
        Self {
            model_path: std::env::var("FASTEMBED_CACHE_DIR").unwrap_or_else(|_| ".fastembed_cache".to_string()),
            model: default_embedder_model(),
            backend: default_embedder_backend(),
            hash_dimension: default_hash_dimension(),
            fallback_to_hash: false,
        }
    }
}

impl Default for HeartbeatSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            base_interval_secs: 1800,
            min_interval_secs: 600,
            max_interval_secs: 3600,
            adaptive: true,
            idle_threshold_secs: 300,
            maintenance_timeout_secs: default_heartbeat_maintenance_timeout_secs(),
            llm_timeout_secs: default_heartbeat_llm_timeout_secs(),
            memory_threshold_mb: default_heartbeat_memory_threshold_mb(),
            max_consecutive_skips: default_heartbeat_max_skips(),
        }
    }
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            data_dir: "./data".to_string(),
            dictionary_dir: "dictionary".to_string(),
            relational_dir: "relational".to_string(),
            vector_dir: "vector_store".to_string(),
            relational_db_file: "relational.db".to_string(),
            vector_index_file: "vectors.hnsw".to_string(),
            question_pool_dir: "heartbeat/questions".to_string(),
        }
    }
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            dir: "./logs".to_string(),
            max_file_size_mb: 10,
        }
    }
}

impl Settings {
    pub fn new() -> Result<Self, config::ConfigError> {
        Self::from_path(None)
    }

    pub fn from_path(config_path: Option<&str>) -> Result<Self, config::ConfigError> {
        let config_path = config_path.map(PathBuf::from).or_else(|| {
            std::env::var("SHOUXIN_CONFIG")
                .ok()
                .map(PathBuf::from)
        }).unwrap_or_else(|| PathBuf::from("config.toml"));

        let local_config_path = config_path
            .parent()
            .map(|p| p.join("config.local.toml"))
            .unwrap_or_else(|| PathBuf::from("config.local.toml"));

        let builder = Config::builder()
            .add_source(config::File::from(config_path.as_path()).required(false))
            .add_source(config::File::from(local_config_path.as_path()).required(false))
            .add_source(
                Environment::with_prefix("SHOUXIN")
                    .prefix_separator("__")
                    .separator("__"),
            );

        let config = builder.build()?;
        // Fix: config crate v0.14 try_deserialize may lose Option<String> fields
        // Manually extract secondary before deserialize (which consumes config)
        let secondary_raw = config.get_string("llm.secondary").ok()
            .filter(|s| !s.is_empty() && s != "none");
        let mut settings: Settings = config.try_deserialize()?;
        if let Some(secondary) = secondary_raw {
            settings.llm.secondary = Some(secondary);
        }
        tracing::info!(
            "Loaded config: LLM primary={}, secondary={:?}",
            settings.llm.primary,
            settings.llm.secondary
        );
        Ok(settings)
    }

    pub fn to_app_config(&self) -> crate::config::AppConfig {
        crate::config::AppConfig {
            llm: LlmConfig {
                url: self.primary_endpoint(),
                model: self.primary_model(),
                temperature: self.llm.temperature,
                max_tokens: self.llm.max_tokens,
            },
            embedder: EmbedderConfig {
                model: self.primary_model(),
                dimension: 768,
                batch_size: 32,
            },
            vector_store: crate::config::vector_store::VectorStoreConfig {
                path: format!("{}/{}", self.storage.data_dir, self.storage.vector_dir),
                dimension: 768,
                persistence: true,
                search_limit: 10,
            },
            data_dir: self.storage.data_dir.clone(),
            dictionary_path: Some(format!("{}/{}", self.storage.data_dir, self.storage.dictionary_dir)),
            relational_path: Some(format!("{}/{}", self.storage.data_dir, self.storage.relational_db_file)),
            vector_path: Some(format!("{}/{}", self.storage.data_dir, self.storage.vector_dir)),
            log_level: self.logging.level.clone(),
        }
    }

    fn primary_model(&self) -> String {
        match self.llm.primary.as_str() {
            "ollama" => self.llm.ollama.as_ref().map(|o| o.model.clone()).unwrap_or_else(|| "llama3.2:3b".to_string()),
            "gguf" => self.llm.gguf.as_ref().map(|g| {
                std::path::Path::new(&g.model_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("gguf-model")
                    .to_string()
            }).unwrap_or_else(|| "gguf-model".to_string()),
            "api" => self.llm.api.as_ref().map(|a| a.model.clone()).unwrap_or_else(|| "api-model".to_string()),
            _ => "llama3.2:3b".to_string(),
        }
    }

    fn primary_endpoint(&self) -> String {
        match self.llm.primary.as_str() {
            "ollama" => self.llm.ollama.as_ref().map(|o| o.endpoint.clone()).unwrap_or_else(|| "http://localhost:11434".to_string()),
            "api" => self.llm.api.as_ref().map(|a| a.endpoint.clone()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    pub fn to_system_config(&self) -> crate::initialization::SystemConfig {
        use crate::stores::vector::factory::VectorStoreConfig as FactoryVectorStoreConfig;
        use crate::stores::vector::factory::VectorStoreType;
        use crate::stores::vector::types::VectorIndexConfig;

        crate::initialization::SystemConfig {
            dictionary_path: format!("{}/{}", self.storage.data_dir, self.storage.dictionary_dir),
            relational_path: format!("{}/{}", self.storage.data_dir, self.storage.relational_db_file),
            vector_config: FactoryVectorStoreConfig {
                path: format!("{}/{}", self.storage.data_dir, self.storage.vector_dir),
                store_type: VectorStoreType::default(),
                dimension: 768,
                index_config: VectorIndexConfig::default(),
            },
            in_memory: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert_eq!(settings.llm.primary, "ollama");
        assert_eq!(settings.heartbeat.base_interval_secs, 1800);
        assert_eq!(settings.storage.data_dir, "./data");
    }

    #[test]
    fn test_to_system_config() {
        let settings = Settings::default();
        let system_config = settings.to_system_config();        assert_eq!(system_config.dictionary_path, "./data/dictionary");
    }
}
/// 仓库内 `config.toml` 的可用性回归
///
/// 新增配置段（如 `[extraction]`、`[todo]`）时最容易出现"代码有字段、配置文件没 key"
/// 或 key 拼错，导致运行时才失败。这里直接解析仓库里的真实配置。
#[cfg(test)]
mod repo_config_tests {
    use super::*;

    #[test]
    fn test_repo_config_file_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.toml");
        let parsed = ::config::Config::builder()
            .add_source(::config::File::from(path))
            .build()
            .expect("config.toml should be valid TOML");
        let settings: Settings = parsed
            .try_deserialize()
            .expect("config.toml should map onto Settings");

        assert!(settings.todo.reminder_threshold > 0);
        assert!(settings.todo.max_pending_before_archive >= settings.todo.reminder_threshold);
        assert!(settings.heartbeat.maintenance_timeout_secs > 0);
        assert!(settings.heartbeat.llm_timeout_secs > 0);
        assert!(settings.extraction.timeout_secs > 0);
    }
}
