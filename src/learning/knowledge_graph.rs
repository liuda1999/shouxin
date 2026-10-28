#![allow(clippy::inherent_to_string)]

//! 知识图谱构建器
//!
//! 抽取实体与关系三元组，支持结构化推理与关联检索

use std::sync::Arc;
use crate::error::ShouxinError;
use crate::traits::LlmClient;
use serde::{Deserialize, Serialize};

/// 知识三元组
///
/// 表示知识图谱中的一个三元组（主语，谓语，宾语）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeTriple {
    /// 主语
    pub subject: String,
    /// 谓语
    pub predicate: String,
    /// 宾语
    pub object: String,
}

impl KnowledgeTriple {
    /// 创建新的知识三元组
    pub fn new(subject: String, predicate: String, object: String) -> Self {
        Self {
            subject,
            predicate,
            object,
        }
    }

    /// 转换为字符串表示
    pub fn to_string(&self) -> String {
        format!("({}, {}, {})", self.subject, self.predicate, self.object)
    }
}

/// 知识图谱抽取器
///
/// 对于每个学习切片，调用 LLM 进行三元组抽取
pub struct KnowledgeGraphExtractor {
    /// LLM 客户端
    llm_client: Arc<dyn LlmClient>,
    /// 每个切片最多抽取的三元组数量
    max_triples_per_slice: usize,
}

impl KnowledgeGraphExtractor {
    /// 创建新的知识图谱抽取器
    ///
    /// # 参数
    /// - `llm_client`: LLM 客户端
    ///
    /// # 返回
    /// 新的知识图谱抽取器实例
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self {
            llm_client,
            max_triples_per_slice: 10,
        }
    }

    /// 创建带有自定义配置的抽取器
    ///
    /// # 参数
    /// - `llm_client`: LLM 客户端
    /// - `max_triples_per_slice`: 每个切片最多抽取的三元组数量
    ///
    /// # 返回
    /// 配置后的抽取器实例
    pub fn with_config(llm_client: Arc<dyn LlmClient>, max_triples_per_slice: usize) -> Self {
        Self {
            llm_client,
            max_triples_per_slice,
        }
    }

    /// 从文本中抽取知识三元组
    ///
    /// # 参数
    /// - `content`: 输入文本
    ///
    /// # 返回
    /// 抽取的三元组列表
    pub async fn extract(&self, content: &str) -> Result<Vec<KnowledgeTriple>, ShouxinError> {
        let prompt = format!(
            r#"从以下文本中抽取知识三元组（主语, 谓语, 宾语），主语和宾语尽量使用归一化后的实体名。
最多抽取 {} 个三元组。

**重要语言约束：所有三元组字段（subject、predicate、object）必须使用中文，禁止使用英语、西班牙语或其他语言。**

文本：{}

输出 JSON 数组格式：[{{"subject": "...", "predicate": "...", "object": "..."}}]"#,
            self.max_triples_per_slice,
            content
        );

        let message = crate::core::Message::user(prompt)
            .map_err(|e| ShouxinError::InvalidArgument(format!("创建消息失败: {}", e)))?;

        let response = self.llm_client.chat(vec![message]).await?;

        let triples: Vec<KnowledgeTriple> = serde_json::from_str(&response)
            .map_err(|e| ShouxinError::InvalidArgument(format!("解析三元组抽取结果失败: {}", e)))?;

        Ok(triples)
    }

    /// 从多个文本中批量抽取三元组
    ///
    /// # 参数
    /// - `contents`: 文本列表
    ///
    /// # 返回
    /// 所有抽取的三元组列表
    pub async fn extract_batch(&self, contents: &[String]) -> Result<Vec<KnowledgeTriple>, ShouxinError> {
        let mut all_triples = Vec::new();

        for content in contents {
            let triples = self.extract(content).await?;
            all_triples.extend(triples);
        }

        Ok(all_triples)
    }

    /// 获取配置信息
    pub fn config(&self) -> KnowledgeGraphConfig {
        KnowledgeGraphConfig {
            max_triples_per_slice: self.max_triples_per_slice,
        }
    }
}

/// 知识图谱抽取器配置
#[derive(Debug, Clone)]
pub struct KnowledgeGraphConfig {
    /// 每个切片最多抽取的三元组数量
    pub max_triples_per_slice: usize,
}

/// 图数据
///
/// 表示知识图谱查询的结果
#[derive(Debug, Clone)]
pub struct GraphData {
    /// 中心实体
    pub center_entity: String,
    /// 关联的三元组列表
    pub triples: Vec<KnowledgeTriple>,
    /// 层级深度
    pub depth: usize,
}

impl GraphData {
    /// 创建新的图数据
    pub fn new(center_entity: String, triples: Vec<KnowledgeTriple>, depth: usize) -> Self {
        Self {
            center_entity,
            triples,
            depth,
        }
    }

    /// 获取三元组数量
    pub fn len(&self) -> usize {
        self.triples.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.triples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_knowledge_triple_creation() {
        let triple = KnowledgeTriple::new(
            "张三".to_string(),
            "就职于".to_string(),
            "阿里".to_string(),
        );
        assert_eq!(triple.subject, "张三");
        assert_eq!(triple.predicate, "就职于");
        assert_eq!(triple.object, "阿里");
    }

    #[test]
    fn test_knowledge_triple_to_string() {
        let triple = KnowledgeTriple::new(
            "北京".to_string(),
            "是".to_string(),
            "中国首都".to_string(),
        );
        assert_eq!(triple.to_string(), "(北京, 是, 中国首都)");
    }

    #[test]
    fn test_graph_data() {
        let triples = vec![
            KnowledgeTriple::new("A".to_string(), "是".to_string(), "B".to_string()),
            KnowledgeTriple::new("B".to_string(), "属于".to_string(), "C".to_string()),
        ];
        let graph = GraphData::new("A".to_string(), triples, 1);
        assert_eq!(graph.len(), 2);
        assert!(!graph.is_empty());
    }
}
