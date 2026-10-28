//! FastEmbed 本地嵌入器实现
//!
//! 使用 fastembed crate 提供纯本地的文本向量化功能
//! 无需联网，支持多种预训练模型

use async_trait::async_trait;
use fastembed::{InitOptionsUserDefined, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel, Pooling};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task;

use crate::core::Embedding;
use crate::error::{ShouxinError, Result};
use crate::traits::embedder::{Embedder, EmbedderConfig};

/// 最大输入文本长度（字符数）
const MAX_INPUT_LENGTH: usize = 100_000;

/// 最大批量处理数量
const MAX_BATCH_SIZE: usize = 1000;

/// FastEmbed 嵌入器
///
/// 使用 fastembed 库的本地嵌入模型
///
/// # 模型信息
/// - 默认模型: BAAI/bge-base-en-v1.5 (768维)
/// - 其他可选模型:
///   - BAAI/bge-small-en-v1.5 (384维)
///   - BAAI/bge-large-en-v1.5 (1024维)
///   - sentence-transformers/all-MiniLM-L6-v2 (384维)
///
/// # 安全特性
/// - 输入验证：限制输入文本长度和批量大小
/// - 错误处理：详细的错误信息，便于调试
///
/// # 性能注意事项
/// - 首次使用会自动下载模型文件（约 40MB - 1GB）
/// - 模型加载后会常驻内存，后续使用很快
/// - 批量处理比单条处理更高效
pub struct FastEmbedder {
    /// FastEmbed 模型实例
    model: Arc<TextEmbedding>,
    /// 配置
    config: EmbedderConfig,
    /// 模型名称
    model_name: String,
}

impl FastEmbedder {
    /// 创建新的 FastEmbedder 实例
    ///
    /// # 参数
    /// - `model_name`: 模型名称，默认为 "BAAI/bge-small-en-v1.5"
    ///
    /// # 返回
    /// 成功返回 FastEmbedder 实例，失败返回 ShouxinError
    ///
    /// # 示例
    /// ```ignore
    /// let embedder = FastEmbedder::new("BAAI/bge-small-en-v1.5")?;
    /// ```
    pub fn new(model_name: &str) -> Result<Self> {
        Self::with_config(EmbedderConfig::new(model_name))
    }

    fn get_cache_dir() -> Option<PathBuf> {
        std::env::var("FASTEMBED_CACHE_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HF_HUB_CACHE")
                    .ok()
                    .map(PathBuf::from)
            })
            .or_else(|| {
                std::env::var("HF_HUB_HOME")
                    .ok()
                    .map(PathBuf::from)
            })
    }

    fn model_exists_in_cache(model_name: &str) -> Option<PathBuf> {
        let cache_dir = Self::get_cache_dir()?;

        let model_parts: Vec<&str> = model_name.split('/').collect();
        let org = model_parts.first().copied().unwrap_or("");
        let model = model_parts.get(1).copied().unwrap_or(model_name);

        // 规范路径: {cache_dir}/{org}/{model}/
        let path = cache_dir.join(org).join(model);

        let onnx_path = path.join("onnx").join("model.onnx");
        if onnx_path.exists() {
            return Some(path);
        }
        let pytorch_path = path.join("pytorch_model.bin");
        if pytorch_path.exists() {
            return Some(path);
        }
        let model_bin_path = path.join("model.bin");
        if model_bin_path.exists() {
            return Some(path);
        }

        None
    }

    fn load_from_cache(model_dir: &Path) -> Result<TextEmbedding> {
        let onnx_path = model_dir.join("onnx").join("model.onnx");
        let onnx_file = fs::read(&onnx_path).map_err(|e| {
            ShouxinError::EmbeddingError(format!("读取模型文件失败: {}", e))
        })?;

        let tokenizer_file = fs::read(model_dir.join("tokenizer.json")).map_err(|e| {
            ShouxinError::EmbeddingError(format!("读取tokenizer.json失败: {}", e))
        })?;
        let config_file = fs::read(model_dir.join("config.json")).map_err(|e| {
            ShouxinError::EmbeddingError(format!("读取config.json失败: {}", e))
        })?;
        let special_tokens_map_file = fs::read(model_dir.join("special_tokens_map.json")).map_err(|e| {
            ShouxinError::EmbeddingError(format!("读取special_tokens_map.json失败: {}", e))
        })?;
        let tokenizer_config_file = fs::read(model_dir.join("tokenizer_config.json")).map_err(|e| {
            ShouxinError::EmbeddingError(format!("读取tokenizer_config.json失败: {}", e))
        })?;

        let tokenizer_files = TokenizerFiles {
            tokenizer_file,
            config_file,
            special_tokens_map_file,
            tokenizer_config_file,
        };

        let user_defined_model = UserDefinedEmbeddingModel::new(onnx_file, tokenizer_files)
            .with_pooling(Pooling::Mean);

        TextEmbedding::try_new_from_user_defined(user_defined_model, InitOptionsUserDefined::default())
            .map_err(|e| {
                ShouxinError::EmbeddingError(format!("从缓存加载模型失败: {}", e))
            })
    }

    /// 使用配置创建 FastEmbedder 实例
    ///
    /// # 参数
    /// - `config`: 嵌入器配置
    ///
    /// # 返回
    /// 成功返回 FastEmbedder 实例，失败返回 ShouxinError
    pub fn with_config(config: EmbedderConfig) -> Result<Self> {
        let model_name = config.model.clone();

        let model = if let Some(model_dir) = Self::model_exists_in_cache(&model_name) {
            println!("从缓存加载模型: {:?}", model_dir);
            Self::load_from_cache(&model_dir)?
        } else {
            println!("从网络下载模型: {}", model_name);
            TextEmbedding::try_new(Default::default()).map_err(|e| {
                ShouxinError::EmbeddingError(format!("初始化嵌入模型失败（模型: {}）: {}", model_name, e))
            })?
        };

        Ok(Self {
            model: Arc::new(model),
            config,
            model_name,
        })
    }

    /// 创建默认的 FastEmbedder 实例
    ///
    /// 使用默认模型 "BAAI/bge-base-en-v1.5"
    ///
    /// # 返回
    /// 成功返回 FastEmbedder 实例，失败返回 ShouxinError
    pub fn new_default() -> Result<Self> {
        Self::with_config(EmbedderConfig::default())
    }

    /// 验证输入文本长度
    ///
    /// # 参数
    /// - text: 要验证的文本
    ///
    /// # 返回
    /// 如果文本长度有效返回 Ok(())，否则返回错误
    fn validate_input_length(text: &str) -> Result<()> {
        if text.is_empty() {
            return Err(ShouxinError::EmbeddingError("输入文本不能为空".to_string()));
        }
        if text.len() > MAX_INPUT_LENGTH {
            return Err(ShouxinError::EmbeddingError(format!(
                "输入文本过长: {} 字符，最大允许 {} 字符",
                text.len(),
                MAX_INPUT_LENGTH
            )));
        }
        Ok(())
    }

    /// 验证批量大小
    ///
    /// # 参数
    /// - texts: 要验证的文本列表
    ///
    /// # 返回
    /// 如果批量大小有效返回 Ok(())，否则返回错误
    fn validate_batch_size(texts: &[String]) -> Result<()> {
        if texts.is_empty() {
            return Err(ShouxinError::EmbeddingError("批量输入不能为空".to_string()));
        }
        if texts.len() > MAX_BATCH_SIZE {
            return Err(ShouxinError::EmbeddingError(format!(
                "批量大小过大: {} 条，最大允许 {} 条",
                texts.len(),
                MAX_BATCH_SIZE
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl Embedder for FastEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        Self::validate_input_length(text)?;

        let text = text.to_string();
        let model = Arc::clone(&self.model);
        let model_name = self.model_name.clone();

        task::spawn_blocking(move || {
            let embeddings = model.embed(vec![&text], None).map_err(|e| {
                ShouxinError::EmbeddingError(format!(
                    "生成嵌入失败（文本长度: {} 字符）: {}",
                    text.len(),
                    e
                ))
            })?;

            let vector = embeddings.into_iter().next().ok_or_else(|| {
                ShouxinError::EmbeddingError(format!(
                    "未生成嵌入向量（模型: {}，文本长度: {}）",
                    model_name,
                    text.len()
                ))
            })?;

            let embedding = Embedding::new(vector, model_name)?;

            Ok(embedding)
        })
        .await
        .map_err(|e| ShouxinError::EmbeddingError(format!("任务执行失败: {}", e)))?
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Embedding>> {
        Self::validate_batch_size(&texts)?;

        // 并行验证输入长度
        let validation: Result<()> = texts
            .par_iter()
            .map(|text| Self::validate_input_length(text))
            .collect();
        validation?;

        let model = Arc::clone(&self.model);
        let model_name = self.model_name.clone();
        let text_count = texts.len();

        task::spawn_blocking(move || -> Result<Vec<Embedding>> {
            let embeddings = model
                .embed(texts.iter().map(|s| s.as_str()).collect(), None)
                .map_err(|e| {
                    ShouxinError::EmbeddingError(format!(
                        "批量生成嵌入失败（数量: {}）: {}",
                        text_count, e
                    ))
                })?;

            if embeddings.len() != text_count {
                return Err(ShouxinError::EmbeddingError(format!(
                    "嵌入数量不匹配：期望 {}，实际 {}",
                    text_count,
                    embeddings.len()
                )));
            }

            // 并行创建嵌入对象
            let results: Result<Vec<Embedding>> = embeddings
                .into_par_iter()
                .enumerate()
                .map(|(i, vector)| {
                    Embedding::new(vector, model_name.clone()).map_err(|e| {
                        ShouxinError::EmbeddingError(format!("创建第 {} 个嵌入失败: {}", i, e))
                    })
                })
                .collect();

            results
        })
        .await
        .map_err(|e| ShouxinError::EmbeddingError(format!("任务执行失败: {}", e)))?
    }

    fn dimension(&self) -> usize {
        768
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn max_input_length(&self) -> usize {
        self.config.max_sequence_length
    }
}
