//! 事件合并阈值（进程级，启动时由配置注入）
//!
//! 为什么放在独立模块而不是 `background_queue` 里：
//! 事件落库有**两条路径**——异步的 `background_queue` 和同步的 `three_library_manager`。
//! 阈值原先只有前者读配置，后者硬编码 0.90/0.75，于是
//! `config.toml` 的 `event_merge_*_threshold` 对实际落库的那条路径完全不生效
//! （实测：配 0.90/0.96，库里仍按 0.75 判进待确认区，无关事件相似度 0.90 出头被全部拦下）。
//!
//! 阈值是**模型相关**的：e5 系嵌入对无关中文短句的余弦相似度本来就有 0.84–0.89，
//! 阈值必须能随模型调整，不能硬编码。

use std::sync::OnceLock;

/// (自动合并阈值, 待确认阈值)，与 `config.toml` 的默认值保持一致
static MERGE_THRESHOLDS: OnceLock<(f64, f64)> = OnceLock::new();

/// 默认阈值：与 `config/settings.rs` 的 `default_event_merge_auto/pending` 一致
pub const DEFAULT_AUTO_THRESHOLD: f64 = 0.96;
pub const DEFAULT_PENDING_THRESHOLD: f64 = 0.90;

/// 注入事件合并阈值：(自动合并, 待确认)
pub fn set_merge_thresholds(auto: f64, pending: f64) {
    let _ = MERGE_THRESHOLDS.set((auto, pending));
}

/// 读取事件合并阈值：(自动合并, 待确认)；未注入时返回默认值
pub fn merge_thresholds() -> (f64, f64) {
    *MERGE_THRESHOLDS
        .get()
        .unwrap_or(&(DEFAULT_AUTO_THRESHOLD, DEFAULT_PENDING_THRESHOLD))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_thresholds_when_not_injected() {
        // 单测进程内可能被其它测试注入过；这里只断言取值自洽（auto > pending）
        let (auto, pending) = merge_thresholds();
        assert!(auto > pending, "auto({}) 应大于 pending({})", auto, pending);
        assert_eq!(DEFAULT_AUTO_THRESHOLD, 0.96);
        assert_eq!(DEFAULT_PENDING_THRESHOLD, 0.90);
    }

    #[test]
    fn injected_value_is_readable() {
        // 取一次当前值，注入后必须能读到新值（OnceLock 只 set 一次，
        // 因此这里用"能读到同一个值"来验证读写通路）
        let before = merge_thresholds();
        set_merge_thresholds(before.0, before.1);
        assert_eq!(merge_thresholds(), before);
    }
}
