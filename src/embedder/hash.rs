//! 离线哈希嵌入器
//!
//! 用途：在没有 ONNX 模型文件、也无法联网下载的环境下，让服务能正常启动并保持
//! **可用的语义检索**（而不是像 `MockEmbedder` 那样返回全零向量）。
//!
//! 原理：字符 n-gram 哈希到固定维度 + 带符号的桶累加 + L2 归一化。
//! 相同/相近文本共享更多 n-gram，余弦相似度更高，因此排序是有意义的；
//! 但它不具备神经嵌入的深层语义能力，**只用于开发/自测/离线降级**，生产应使用
//! `backend = "fastembed"`。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use async_trait::async_trait;

use crate::core::Embedding;
use crate::error::ShouxinError;
use crate::traits::embedder::Embedder;

/// 默认维度（与项目默认嵌入模型 multilingual-e5-base 对齐）
pub const DEFAULT_HASH_DIMENSION: usize = 768;

/// 基于字符 n-gram 哈希的离线嵌入器
pub struct HashEmbedder {
    dimension: usize,
    model_name: String,
}

impl HashEmbedder {
    pub fn new(dimension: usize, model_name: &str) -> Self {
        Self {
            dimension: dimension.max(1),
            model_name: model_name.to_string(),
        }
    }

    pub fn new_default() -> Self {
        Self::new(DEFAULT_HASH_DIMENSION, "hash-ngram-768")
    }

    /// 计算单个文本的向量（已归一化）
    fn compute(&self, text: &str) -> Vec<f32> {
        let mut vec = vec![0.0f32; self.dimension];

        let normalized: String = text
            .to_lowercase()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let chars: Vec<char> = normalized.chars().collect();

        if chars.is_empty() {
            // 空文本返回单位向量，避免零向量导致余弦相似度无定义
            vec[0] = 1.0;
            return vec;
        }

        // unigram：保留单字信息（中文场景很重要）
        for ch in &chars {
            self.accumulate(&mut vec, &ch.to_string(), 1.0);
        }

        // bigram / trigram：捕获词组与顺序
        for window in 2..=3usize {
            if chars.len() < window {
                continue;
            }
            let weight = if window == 2 { 1.5 } else { 2.0 };
            for start in 0..=(chars.len() - window) {
                let gram: String = chars[start..start + window].iter().collect();
                self.accumulate(&mut vec, &gram, weight);
            }
        }

        l2_normalize(&mut vec);
        vec
    }

    /// 把一个 n-gram 累加到对应桶（带符号，减少哈希碰撞的系统性偏置）
    fn accumulate(&self, vec: &mut [f32], gram: &str, weight: f32) {
        let mut hasher = DefaultHasher::new();
        gram.hash(&mut hasher);
        let h = hasher.finish();

        let idx = (h % self.dimension as u64) as usize;
        let sign = if (h >> 63) & 1 == 0 { 1.0 } else { -1.0 };
        vec[idx] += sign * weight;
    }
}

/// 原地 L2 归一化；零向量兜底为单位向量
fn l2_normalize(vec: &mut [f32]) {
    let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 1e-12 {
        for v in vec.iter_mut() {
            *v /= norm;
        }
    } else if let Some(first) = vec.first_mut() {
        *first = 1.0;
    }
}

#[async_trait]
impl Embedder for HashEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding, ShouxinError> {
        Ok(Embedding {
            vector: self.compute(text),
            dimension: self.dimension,
            model_name: self.model_name.clone(),
        })
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Embedding>, ShouxinError> {
        Ok(texts
            .iter()
            .map(|t| Embedding {
                vector: self.compute(t),
                dimension: self.dimension,
                model_name: self.model_name.clone(),
            })
            .collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn max_input_length(&self) -> usize {
        // 哈希嵌入不截断，给一个宽松上界即可
        usize::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[tokio::test]
    async fn test_deterministic_and_normalized() {
        let e = HashEmbedder::new_default();
        let a = e.embed("守心，是我的长期记忆系统").await.unwrap();
        let b = e.embed("守心，是我的长期记忆系统").await.unwrap();

        assert_eq!(a.vector, b.vector, "同一文本必须得到完全一致的向量");
        assert_eq!(a.dimension, 768);

        let norm: f32 = a.vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "向量应已 L2 归一化，实际 {}", norm);
    }

    #[tokio::test]
    async fn test_similar_text_scores_higher_than_unrelated() {
        let e = HashEmbedder::new_default();
        let base = e.embed("明天下午三点和张总开会讨论预算").await.unwrap();
        let similar = e.embed("明天下午三点和张总开会").await.unwrap();
        let unrelated = e.embed("猫咪喜欢在阳台上晒太阳").await.unwrap();

        let s1 = cosine(&base.vector, &similar.vector);
        let s2 = cosine(&base.vector, &unrelated.vector);

        assert!(
            s1 > s2,
            "相近文本相似度({})应高于无关文本({})",
            s1,
            s2
        );
    }

    #[tokio::test]
    async fn test_empty_text_is_safe() {
        let e = HashEmbedder::new_default();
        let v = e.embed("").await.unwrap();
        let norm: f32 = v.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "空文本也不应产生零向量");
    }
}
