//! 事件元信息分类
//!
//! 重要性分级与情感倾向判定在两条写入路径上都要用到：
//! - 同步路径：`memory_extractor::dispatcher`
//! - 异步路径：`service::background_queue`
//!
//! 集中在此作为**唯一来源**，避免两条路径各自维护一份阈值/关键词导致行为漂移。

/// 重要性达到该分值视为 critical（永不自动失效）
pub const IMPORTANCE_CRITICAL: f32 = 0.95;
/// 重要性高于该分值视为 long_term
pub const IMPORTANCE_LONG_TERM: f32 = 0.7;

/// 负面情感关键词（低成本启发式，后续可由 LLM 情感分析替换）
pub const NEGATIVE_KEYWORDS: &[&str] = &[
    "弄丢", "丢失", "忘记", "错过", "失败", "错误", "问题", "麻烦",
];

/// 正面情感关键词
pub const POSITIVE_KEYWORDS: &[&str] = &[
    "成功", "完成", "获得", "赢得", "优秀", "出色", "很好", "不错",
];

/// 将 0.0–1.0 的重要性分数映射为重要性级别字符串
pub fn classify_importance(importance_score: f32) -> &'static str {
    if importance_score >= IMPORTANCE_CRITICAL {
        "critical"
    } else if importance_score > IMPORTANCE_LONG_TERM {
        "long_term"
    } else {
        "short_term"
    }
}

/// 基于关键词粗略判断文本的情感倾向
///
/// 返回 `positive` / `negative` / `neutral`。
pub fn analyze_sentiment(text: &str) -> &'static str {
    let lowered = text.to_lowercase();

    if NEGATIVE_KEYWORDS.iter().any(|kw| lowered.contains(kw)) {
        return "negative";
    }
    if POSITIVE_KEYWORDS.iter().any(|kw| lowered.contains(kw)) {
        return "positive";
    }
    "neutral"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_importance() {
        assert_eq!(classify_importance(0.96), "critical");
        assert_eq!(classify_importance(0.95), "critical");
        assert_eq!(classify_importance(0.8), "long_term");
        assert_eq!(classify_importance(0.7), "short_term");
        assert_eq!(classify_importance(0.1), "short_term");
    }

    #[test]
    fn test_analyze_sentiment() {
        assert_eq!(analyze_sentiment("他弄丢了书"), "negative");
        assert_eq!(analyze_sentiment("任务顺利完成"), "positive");
        assert_eq!(analyze_sentiment("今天一起吃饭"), "neutral");
    }
}
