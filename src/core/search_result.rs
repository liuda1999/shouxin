//! 搜索结果数据结构
//!
//! 表示记忆搜索的结果

use serde::{Deserialize, Serialize};
use std::str::FromStr;

use super::memory::Memory;

/// 搜索来源类型
///
/// 标识搜索结果的来源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SearchSource {
    /// 向量搜索
    ///
    /// 基于嵌入向量的语义相似度搜索
    #[default]
    Vector,

    /// 图搜索
    ///
    /// 基于知识图谱的关系搜索
    Graph,

    /// 混合搜索
    ///
    /// 结合向量和图搜索的结果
    Hybrid,

    /// 关键词搜索
    ///
    /// 基于关键词的全文搜索
    Keyword,
}

impl SearchSource {
    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            SearchSource::Vector => "vector",
            SearchSource::Graph => "graph",
            SearchSource::Hybrid => "hybrid",
            SearchSource::Keyword => "keyword",
        }
    }
}

impl FromStr for SearchSource {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "vector" => Ok(SearchSource::Vector),
            "graph" => Ok(SearchSource::Graph),
            "hybrid" => Ok(SearchSource::Hybrid),
            "keyword" => Ok(SearchSource::Keyword),
            _ => Ok(SearchSource::Vector),
        }
    }
}

/// 搜索结果
///
/// 表示单条搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// 匹配的记忆
    pub memory: Memory,

    /// 相关性分数
    ///
    /// 范围 [0.0, 1.0]，越高表示越相关
    pub score: f32,

    /// 搜索来源
    pub source: SearchSource,

    /// 匹配的文本片段
    ///
    /// 可选的匹配文本高亮片段
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_text: Option<String>,

    /// 额外的上下文信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

impl SearchResult {
    /// 创建新的搜索结果
    ///
    /// # 参数
    /// - `memory`: 匹配的记忆
    /// - `score`: 相关性分数
    /// - `source`: 搜索来源
    ///
    /// # 返回
    /// 新的 SearchResult 实例
    pub fn new(memory: Memory, score: f32, source: SearchSource) -> Self {
        Self {
            memory,
            score,
            source,
            matched_text: None,
            context: None,
        }
    }

    /// 创建向量搜索结果
    pub fn from_vector(memory: Memory, score: f32) -> Self {
        Self::new(memory, score, SearchSource::Vector)
    }

    /// 创建图搜索结果
    pub fn from_graph(memory: Memory, score: f32) -> Self {
        Self::new(memory, score, SearchSource::Graph)
    }

    /// 创建混合搜索结果
    pub fn from_hybrid(memory: Memory, score: f32) -> Self {
        Self::new(memory, score, SearchSource::Hybrid)
    }

    /// 设置匹配文本
    pub fn with_matched_text(mut self, text: impl Into<String>) -> Self {
        self.matched_text = Some(text.into());
        self
    }

    /// 设置上下文信息
    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = Some(context);
        self
    }

    /// 检查是否为高相关性结果
    ///
    /// 分数 >= 0.7 视为高相关性
    pub fn is_highly_relevant(&self) -> bool {
        self.score >= 0.7
    }
}

/// 搜索结果集合
///
/// 管理一组搜索结果
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchResults {
    /// 搜索结果列表
    pub results: Vec<SearchResult>,

    /// 搜索耗时（毫秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,

    /// 总匹配数
    ///
    /// 可能大于返回的结果数量
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
}

impl SearchResults {
    /// 创建空的搜索结果集合
    pub fn new() -> Self {
        Self::default()
    }

    /// 从结果列表创建
    pub fn from_results(results: Vec<SearchResult>) -> Self {
        let total = Some(results.len());
        Self {
            results,
            took_ms: None,
            total,
        }
    }

    /// 添加搜索结果
    pub fn add(&mut self, result: SearchResult) {
        self.results.push(result);
    }

    /// 设置搜索耗时
    pub fn with_took_ms(mut self, took_ms: u64) -> Self {
        self.took_ms = Some(took_ms);
        self
    }

    /// 设置总数
    pub fn with_total(mut self, total: usize) -> Self {
        self.total = Some(total);
        self
    }

    /// 按分数排序（降序）
    pub fn sort_by_score(&mut self) {
        self.results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// 获取高相关性结果
    pub fn highly_relevant(&self) -> Vec<&SearchResult> {
        self.results
            .iter()
            .filter(|r| r.is_highly_relevant())
            .collect()
    }

    /// 获取结果数量
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// 限制结果数量
    pub fn limit(&mut self, n: usize) {
        self.results.truncate(n);
    }
}

/// 搜索查询
///
/// 定义搜索请求的参数
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    /// 查询文本
    pub query: String,

    /// 查询嵌入向量
    ///
    /// 可选，如果提供则跳过嵌入生成
    pub query_embedding: Option<Vec<f32>>,

    /// 返回结果数量限制
    pub limit: usize,

    /// 最小相关性分数阈值
    pub min_score: f32,

    /// 搜索来源
    pub source: SearchSource,

    /// 用户 ID 过滤
    pub user_id: Option<String>,

    /// 代理 ID 过滤
    pub agent_id: Option<String>,
}

impl SearchQuery {
    /// 创建新的搜索查询
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            query_embedding: None,
            limit: 10,
            min_score: 0.0,
            source: SearchSource::Vector,
            user_id: None,
            agent_id: None,
        }
    }

    /// 设置结果数量限制
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// 设置最小分数阈值
    pub fn with_min_score(mut self, min_score: f32) -> Self {
        self.min_score = min_score;
        self
    }

    /// 设置搜索来源
    pub fn with_source(mut self, source: SearchSource) -> Self {
        self.source = source;
        self
    }

    /// 设置用户 ID 过滤
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// 设置查询嵌入向量
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.query_embedding = Some(embedding);
        self
    }
}
