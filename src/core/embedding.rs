//! 嵌入向量数据结构
//!
//! 表示文本的向量嵌入，用于语义搜索和相似度计算

use serde::{Deserialize, Serialize};

use crate::utils;

/// 嵌入向量
///
/// 存储文本的向量表示及其元信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Embedding {
    /// 向量数据
    ///
    /// 浮点数数组，表示文本在高维空间中的位置
    pub vector: Vec<f32>,

    /// 向量维度
    ///
    /// 向量的长度，取决于使用的嵌入模型
    /// 常见维度：384 (all-MiniLM-L6-v2), 768 (BAAI/bge-base-en-v1.5), 1536 (OpenAI)
    pub dimension: usize,

    /// 模型名称
    ///
    /// 生成此嵌入的模型名称，如 "BAAI/bge-small-en-v1.5"
    pub model_name: String,
}

impl Embedding {
    /// 创建新的嵌入向量
    ///
    /// # 参数
    /// - `vector`: 向量数据，不能为空且所有元素必须是有效浮点数（非 NaN、非 Inf）
    /// - `model_name`: 模型名称，不能为空
    ///
    /// # 返回
    /// 新的 Embedding 实例，维度自动从向量长度推断
    ///
    /// # 错误
    /// 如果向量为空、包含无效浮点数或模型名称为空，返回错误
    pub fn new(vector: Vec<f32>, model_name: String) -> anyhow::Result<Self> {
        utils::validate_vector(&vector, "Vector")?;
        utils::validate_non_empty(&model_name, "Model name")?;

        let dimension = vector.len();
        Ok(Self {
            vector,
            dimension,
            model_name,
        })
    }

    /// 计算与另一个嵌入向量的余弦相似度
    ///
    /// # 参数
    /// - `other`: 另一个嵌入向量
    ///
    /// # 返回
    /// 余弦相似度值，范围 [-1, 1]，1 表示完全相同
    pub fn cosine_similarity(&self, other: &Embedding) -> f32 {
        if self.dimension != other.dimension {
            return 0.0;
        }

        let dot_product: f32 = self
            .vector
            .iter()
            .zip(other.vector.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm_a: f32 = self.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = other.vector.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot_product / (norm_a * norm_b)
        }
    }

    /// 检查嵌入向量是否为空
    pub fn is_empty(&self) -> bool {
        self.vector.is_empty()
    }
}
