//! Embedder 工厂类
//!
//! 提供统一的 Embedder 创建接口，支持多种嵌入模型

use super::fastembed::FastEmbedder;
use crate::error::Result;
use crate::traits::embedder::{Embedder, EmbedderConfig};

/// 嵌入器类型
///
/// 支持的嵌入器类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedderType {
    /// FastEmbed 本地嵌入器
    FastEmbed,
}

/// Embedder 工厂
///
/// 提供统一的 Embedder 创建接口
pub struct EmbedderFactory;

impl EmbedderFactory {
    /// 创建默认的 Embedder
    ///
    /// 使用 FastEmbed 作为默认嵌入器
    ///
    /// # 返回
    /// Box<dyn Embedder> 类型的嵌入器
    pub fn create_default() -> Result<Box<dyn Embedder>> {
        Self::create(EmbedderType::FastEmbed, EmbedderConfig::default())
    }

    /// 根据类型和配置创建 Embedder
    ///
    /// # 参数
    /// - `embedder_type`: 嵌入器类型
    /// - `config`: 嵌入器配置
    ///
    /// # 返回
    /// Box<dyn Embedder> 类型的嵌入器
    pub fn create(
        embedder_type: EmbedderType,
        config: EmbedderConfig,
    ) -> Result<Box<dyn Embedder>> {
        match embedder_type {
            EmbedderType::FastEmbed => {
                let embedder = FastEmbedder::with_config(config)?;
                Ok(Box::new(embedder))
            }
        }
    }

    /// 创建 FastEmbedder
    ///
    /// # 参数
    /// - `config`: 嵌入器配置
    ///
    /// # 返回
    /// Box<dyn Embedder> 类型的 FastEmbedder
    pub fn create_fastembed(config: EmbedderConfig) -> Result<Box<dyn Embedder>> {
        let embedder = FastEmbedder::with_config(config)?;
        Ok(Box::new(embedder))
    }

    /// 根据模型名称创建 Embedder
    ///
    /// 自动推断嵌入器类型（目前仅支持 FastEmbed）
    ///
    /// # 参数
    /// - `model_name`: 模型名称
    ///
    /// # 返回
    /// Box<dyn Embedder> 类型的嵌入器
    pub fn from_model_name(model_name: &str) -> Result<Box<dyn Embedder>> {
        let config = EmbedderConfig::new(model_name);
        Self::create(EmbedderType::FastEmbed, config)
    }
}
