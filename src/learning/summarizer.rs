#![allow(clippy::manual_pattern_char_comparison)]

//! 关键词与摘要生成器
//!
//! 调用 LLM 生成摘要与关键词，优化关键词检索与快速预览

use std::sync::Arc;
use crate::error::ShouxinError;
use crate::traits::LlmClient;

/// 摘要生成结果
#[derive(Debug, Clone)]
pub struct SummaryResult {
    /// 一句话摘要（≤ 50 字）
    pub summary: String,
    /// 提取的关键词列表
    pub keywords: Vec<String>,
}

/// 摘要与关键词生成器
///
/// 在切片入库前，调用 LLM 生成摘要和关键词
pub struct Summarizer {
    /// LLM 客户端
    llm_client: Arc<dyn LlmClient>,
    /// 摘要最大长度
    max_summary_length: usize,
    /// 关键词最小数量
    min_keywords: usize,
    /// 关键词最大数量
    max_keywords: usize,
}

impl Summarizer {
    /// 创建新的摘要生成器
    ///
    /// # 参数
    /// - `llm_client`: LLM 客户端
    ///
    /// # 返回
    /// 新的摘要生成器实例
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self {
            llm_client,
            max_summary_length: 50,
            min_keywords: 3,
            max_keywords: 5,
        }
    }

    /// 创建带有自定义配置的摘要生成器
    ///
    /// # 参数
    /// - `llm_client`: LLM 客户端
    /// - `max_summary_length`: 摘要最大长度
    /// - `min_keywords`: 关键词最小数量
    /// - `max_keywords`: 关键词最大数量
    ///
    /// # 返回
    /// 配置后的摘要生成器实例
    pub fn with_config(
        llm_client: Arc<dyn LlmClient>,
        max_summary_length: usize,
        min_keywords: usize,
        max_keywords: usize,
    ) -> Self {
        Self {
            llm_client,
            max_summary_length,
            min_keywords,
            max_keywords,
        }
    }

    /// 生成摘要和关键词
    ///
    /// # 参数
    /// - `content`: 输入内容
    ///
    /// # 返回
    /// 摘要生成结果
    pub async fn summarize(&self, content: &str) -> Result<SummaryResult, ShouxinError> {
        let prompt = format!(
            r#"为以下文本生成一句话摘要（不超过{}字）和{}-{}个关键词。

**重要语言约束：摘要和关键词必须使用中文，禁止使用英语、西班牙语或其他语言。**

文本：{}

输出 JSON 格式：{{"summary": "...", "keywords": ["...", "..."]}}"#,
            self.max_summary_length,
            self.min_keywords,
            self.max_keywords,
            content
        );

        let message = crate::core::Message::user(prompt)
            .map_err(|e| ShouxinError::InvalidArgument(format!("创建消息失败: {}", e)))?;

        let response = self.llm_client.chat(vec![message]).await?;

        let result: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| ShouxinError::InvalidArgument(format!("解析摘要生成结果失败: {}", e)))?;

        let summary = result["summary"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let keywords: Vec<String> = result["keywords"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(SummaryResult { summary, keywords })
    }

    /// 仅生成摘要
    ///
    /// # 参数
    /// - `content`: 输入内容
    ///
    /// # 返回
    /// 摘要字符串
    pub async fn generate_summary(&self, content: &str) -> Result<String, ShouxinError> {
        let prompt = format!(
            r#"为以下文本生成一句话摘要（不超过{}字）。

**重要语言约束：摘要必须使用中文，禁止使用英语、西班牙语或其他语言。**

文本：{}

直接输出摘要，不要其他内容："#,
            self.max_summary_length, content
        );

        let message = crate::core::Message::user(prompt)
            .map_err(|e| ShouxinError::InvalidArgument(format!("创建消息失败: {}", e)))?;

        let response = self.llm_client.chat(vec![message]).await?;

        Ok(response.trim().to_string())
    }

    /// 仅生成关键词
    ///
    /// # 参数
    /// - `content`: 输入内容
    ///
    /// # 返回
    /// 关键词列表
    pub async fn extract_keywords(&self, content: &str) -> Result<Vec<String>, ShouxinError> {
        let prompt = format!(
            r#"从以下文本中提取{}-{}个关键词，用 JSON 数组格式输出。

**重要语言约束：关键词必须使用中文，禁止使用英语、西班牙语或其他语言。**

文本：{}

输出格式：["关键词1", "关键词2", ...]"#,
            self.min_keywords, self.max_keywords, content
        );

        let message = crate::core::Message::user(prompt)
            .map_err(|e| ShouxinError::InvalidArgument(format!("创建消息失败: {}", e)))?;

        let response = self.llm_client.chat(vec![message]).await?;

        let keywords: Vec<String> = serde_json::from_str(&response)
            .unwrap_or_else(|_| {
                response
                    .trim()
                    .split(|c: char| c == ',' || c == '[' || c == ']' || c == '"')
                    .filter(|s| !s.trim().is_empty())
                    .map(String::from)
                    .collect()
            });

        Ok(keywords)
    }

    /// 获取配置信息
    pub fn config(&self) -> SummarizerConfig {
        SummarizerConfig {
            max_summary_length: self.max_summary_length,
            min_keywords: self.min_keywords,
            max_keywords: self.max_keywords,
        }
    }
}

/// 摘要生成器配置
#[derive(Debug, Clone)]
pub struct SummarizerConfig {
    /// 摘要最大长度
    pub max_summary_length: usize,
    /// 关键词最小数量
    pub min_keywords: usize,
    /// 关键词最大数量
    pub max_keywords: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarizer_config() {
        let config = SummarizerConfig {
            max_summary_length: 50,
            min_keywords: 3,
            max_keywords: 5,
        };
        assert_eq!(config.max_summary_length, 50);
        assert_eq!(config.min_keywords, 3);
        assert_eq!(config.max_keywords, 5);
    }
}
