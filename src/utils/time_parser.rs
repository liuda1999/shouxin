//! 时间解析工具模块
//!
//! 提供自然语言时间表达式解析功能

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use regex::Regex;
use std::collections::HashMap;

pub fn parse_event_time(text: &str, reference_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    if let Some(dt) = parse_month_edge(text, reference_time) {
        return Some(dt);
    }

    if let Some(dt) = parse_absolute_time(text, reference_time) {
        return Some(dt);
    }

    if let Some(dt) = parse_relative_time(text, reference_time) {
        return Some(dt);
    }

    if let Some(dt) = parse_weekday_time(text, reference_time) {
        return Some(dt);
    }

    if let Some(dt) = parse_fuzzy_time(text, reference_time) {
        return Some(dt);
    }

    None
}

fn parse_absolute_time(text: &str, reference_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let today = reference_time.date_naive();

    // 说明：原实现按 `caps.len()` 分派，导致 "6月15日"（3 组）被当成 06:15；
    // 且 `4 if` 守卫分支永远不可达。这里改为"每个形态独立匹配"，语义显式、无歧义。

    // 1) 完整日期 + 时间：2026-06-03 15:00 / 2026年6月3日 15:00
    if let Some(caps) = re_ymd_hm().captures(text) {
        let year = caps.get(1)?.as_str().parse::<i32>().ok()?;
        let month = caps.get(2)?.as_str().parse::<u32>().ok()?;
        let day = caps.get(3)?.as_str().parse::<u32>().ok()?;
        let hour = caps.get(4)?.as_str().parse::<u32>().ok()?;
        let minute = caps.get(5)?.as_str().parse::<u32>().ok()?;
        return build_datetime(year, month, day, hour, minute);
    }

    // 2) 完整日期：2026-06-10
    if let Some(caps) = re_ymd().captures(text) {
        let year = caps.get(1)?.as_str().parse::<i32>().ok()?;
        let month = caps.get(2)?.as_str().parse::<u32>().ok()?;
        let day = caps.get(3)?.as_str().parse::<u32>().ok()?;
        return build_datetime(year, month, day, 0, 0);
    }

    // 3) 月日 + 时间：6月15日 14:30
    if let Some(caps) = re_md_hm().captures(text) {
        let month = caps.get(1)?.as_str().parse::<u32>().ok()?;
        let day = caps.get(2)?.as_str().parse::<u32>().ok()?;
        let hour = caps.get(3)?.as_str().parse::<u32>().ok()?;
        let minute = caps.get(4)?.as_str().parse::<u32>().ok()?;
        let year = infer_year(today, month, day);
        return build_datetime(year, month, day, hour, minute);
    }

    // 4) 月日：6月15日 / 6-15
    if let Some(caps) = re_md().captures(text) {
        let month = caps.get(1)?.as_str().parse::<u32>().ok()?;
        let day = caps.get(2)?.as_str().parse::<u32>().ok()?;
        let year = infer_year(today, month, day);
        return build_datetime(year, month, day, 0, 0);
    }

    // 5) 今天/明天 + 时间：今天 15:30 / 明天 09:00
    if let Some(caps) = re_today_hm().captures(text) {
        let is_tomorrow = text.contains("明天");
        let hour = caps.get(1)?.as_str().parse::<u32>().ok()?;
        let minute = caps.get(2)?.as_str().parse::<u32>().ok()?;
        let target_date = if is_tomorrow {
            today + chrono::Duration::days(1)
        } else {
            today
        };
        return NaiveTime::from_hms_opt(hour, minute, 0)
            .map(|time| Utc.from_utc_datetime(&target_date.and_time(time)));
    }

    // 6) 纯时间：15:30（落在参考日当天）
    if let Some(caps) = re_hm().captures(text) {
        let hour = caps.get(1)?.as_str().parse::<u32>().ok()?;
        let minute = caps.get(2)?.as_str().parse::<u32>().ok()?;
        return NaiveTime::from_hms_opt(hour, minute, 0)
            .map(|time| Utc.from_utc_datetime(&today.and_time(time)));
    }

    None
}

/// 若"月日"已过去，则归入下一年
fn infer_year(today: NaiveDate, month: u32, day: u32) -> i32 {
    let is_future_or_today = (month, day) >= (today.month(), today.day());
    if is_future_or_today {
        today.year()
    } else {
        today.year() + 1
    }
}

fn build_datetime(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
) -> Option<DateTime<Utc>> {
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
    Some(Utc.from_utc_datetime(&date.and_time(time)))
}

macro_rules! static_regex {
    ($name:ident, $pattern:expr) => {
        fn $name() -> &'static Regex {
            static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
            RE.get_or_init(|| Regex::new($pattern).expect("static regex must be valid"))
        }
    };
}

static_regex!(
    re_ymd_hm,
    r"(\d{4})\s*[年\-/]\s*(\d{1,2})\s*[月\-/]\s*(\d{1,2})\s*日?\s+(\d{1,2})[:：](\d{2})"
);
static_regex!(
    re_ymd,
    r"(\d{4})\s*[年\-/]\s*(\d{1,2})\s*[月\-/]\s*(\d{1,2})\s*日?"
);
static_regex!(
    re_md_hm,
    r"(\d{1,2})\s*[月\-/]\s*(\d{1,2})\s*日?\s+(\d{1,2})[:：](\d{2})"
);
static_regex!(re_md, r"(\d{1,2})\s*[月\-/]\s*(\d{1,2})\s*日?");
static_regex!(
    re_today_hm,
    r"(?:今天|明天)\s*(\d{1,2})\s*[:：]\s*(\d{2})"
);
static_regex!(re_hm, r"(\d{1,2})\s*[:：]\s*(\d{2})");
static_regex!(re_clock_point, r"(\d{1,2})\s*[点時时]");

/// 时段词表（**有序**，长词/更具体的词优先）
///
/// 第三项表示该时段属于"下午侧"：当出现 `N点` 且 N < 12 时需要 +12。
const MERIDIEM_HINTS: &[(&str, u32, bool)] = &[
    ("凌晨", 3, false),
    ("早上", 8, false),
    ("早晨", 8, false),
    ("上午", 9, false),
    ("中午", 12, false),
    ("下午", 14, true),
    ("傍晚", 18, true),
    ("晚上", 20, true),
    ("夜里", 22, true),
];

/// 从文本中抽取时间
///
/// 语义：
/// - 有显式 `N点` / `N:MM` 时以显式值为准，但会被时段词修正（如"下午2点" -> 14:00）；
/// - 只有时段词时取该时段的代表时刻；
/// - 都没有时回退到调用方给的默认值。
fn extract_time_from_text(text: &str, default_hour: u32, default_minute: u32) -> (u32, u32) {
    let explicit = parse_explicit_clock(text);
    let modifier = MERIDIEM_HINTS
        .iter()
        .find(|(keyword, _, _)| text.contains(keyword));

    match (explicit, modifier) {
        (Some((hour, minute)), Some((_, _, is_pm))) => {
            let hour = if *is_pm && hour < 12 { hour + 12 } else { hour };
            (hour, minute)
        }
        (Some((hour, minute)), None) => (hour, minute),
        (None, Some((_, base_hour, _))) => (*base_hour, 0),
        (None, None) => (default_hour, default_minute),
    }
}

/// 解析显式时刻（`15:30` 或 `2点` / `8时`）
fn parse_explicit_clock(text: &str) -> Option<(u32, u32)> {
    if let Some(caps) = re_hm().captures(text) {
        let hour = caps.get(1)?.as_str().parse::<u32>().ok()?;
        let minute = caps.get(2)?.as_str().parse::<u32>().ok()?;
        if hour <= 23 && minute <= 59 {
            return Some((hour, minute));
        }
    }
    if let Some(caps) = re_clock_point().captures(text) {
        let hour = caps.get(1)?.as_str().parse::<u32>().ok()?;
        if hour <= 23 {
            return Some((hour, 0));
        }
    }
    None
}


static_regex!(re_days_later, r"(\d+)\s*天后?");

/// 解析相对日期：今天 / 明天 / 后天 / N天后，并叠加时段
/// 解析"月底 / 月初"这类按月边界表达
///
/// 之前 `find_precise_time_expr` 会把"月底"匹配出来，但解析器不支持它，
/// 于是"月底前出一版方案"拿不到日期（或被别的句子带偏成"今天"）。
fn parse_month_edge(text: &str, reference_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
    use chrono::Datelike;

    let today = reference_time.date_naive();
    let (year, month) = (today.year(), today.month());

    let day = if text.contains("月底") || text.contains("月末") {
        // 当月最后一天：下月 1 号减一天
        let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        chrono::NaiveDate::from_ymd_opt(ny, nm, 1)? - chrono::Duration::days(1)
    } else if text.contains("月初") {
        chrono::NaiveDate::from_ymd_opt(year, month, 1)?
    } else {
        return None;
    };

    let (hour, minute) = extract_time_from_text(text, 23, 59);
    chrono::NaiveTime::from_hms_opt(hour, minute, 0)
        .map(|t| Utc.from_utc_datetime(&day.and_time(t)))
}

fn parse_relative_time(text: &str, reference_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let today = reference_time.date_naive();

    let (day_offset, default_hour, default_minute) =
        if let Some(caps) = re_days_later().captures(text) {
            let days = caps.get(1)?.as_str().parse::<i64>().ok()?;
            (days, 9, 0)
        } else if text.contains("今天") || text.contains("今日") {
            (0, reference_time.hour(), reference_time.minute())
        } else if text.contains("明天") || text.contains("明日") {
            (1, 9, 0)
        } else if text.contains("后天") {
            (2, 9, 0)
        } else {
            return None;
        };

    let target_date = today + chrono::Duration::days(day_offset);
    let (hour, minute) = extract_time_from_text(text, default_hour, default_minute);
    NaiveTime::from_hms_opt(hour, minute, 0)
        .map(|time| Utc.from_utc_datetime(&target_date.and_time(time)))
}

/// 星期词表（有序；"天"与"日"都表示周日）
const WEEKDAY_HINTS: &[(&str, i64)] = &[
    ("一", 0),
    ("二", 1),
    ("三", 2),
    ("四", 3),
    ("五", 4),
    ("六", 5),
    ("日", 6),
    ("天", 6),
];

/// 解析星期表达：周三 / 下周三 / 本周三 / 星期三 / 下星期三
///
/// 原实现把"下周/本周"的判断写成 `pattern.starts_with("下")`，而词表里没有"下"
/// 前缀，分支永远不成立，导致"下周三"被解析成"最近的那个周三"。这里显式处理。
fn parse_weekday_time(text: &str, reference_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let today = reference_time.date_naive();
    let days_since_monday = today.weekday().num_days_from_monday() as i64;

    let target_offset = WEEKDAY_HINTS.iter().find_map(|(suffix, offset)| {
        if text.contains(&format!("周{}", suffix)) || text.contains(&format!("星期{}", suffix)) {
            Some(*offset)
        } else {
            None
        }
    })?;

    let next_week = text.contains("下周") || text.contains("下星期");
    let this_week = text.contains("本周") || text.contains("这周") || text.contains("本星期");

    let days_until = if next_week {
        // 下周的同一星期几：距离"下周一"还有 (7 - days_since_monday) 天，再加上目标星期几的偏移。
        // 例：今天周三(2) → 下周一 = 7-2+0 = 5 天后；下周三 = 7-2+2 = 7 天后。
        // （旧公式 `(offset - since + 7) % 7 + 7` 会多算一周，把"下周一"推到 11-09）
        (7 - days_since_monday + target_offset).max(1)
    } else if this_week {
        // 本周：允许指向已经过去的那一天
        target_offset - days_since_monday
    } else {
        // 未限定：取最近将要到来的那一天；今天就是则顺延一周
        let base = (target_offset - days_since_monday + 7) % 7;
        if base == 0 {
            7
        } else {
            base
        }
    };

    let target_date = today + chrono::Duration::days(days_until);
    let (hour, minute) = extract_time_from_text(text, 10, 0);
    NaiveTime::from_hms_opt(hour, minute, 0)
        .map(|time| Utc.from_utc_datetime(&target_date.and_time(time)))
}


fn parse_fuzzy_time(text: &str, reference_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let today = reference_time.date_naive();
    let fuzzy_patterns: HashMap<&str, (i64, u32, u32)> = HashMap::from([
        ("今晚", (0, 20, 0)),
        ("明早", (1, 8, 0)),
        ("明晚", (1, 20, 0)),
        ("稍后", (0, 14, 0)),
        ("待会", (0, 14, 0)),
        ("一会儿", (0, 14, 0)),
        ("尽快", (0, 12, 0)),
        ("立刻", (0, 12, 0)),
        ("马上", (0, 12, 0)),
    ]);

    for (pattern, (days_offset, hour, minute)) in &fuzzy_patterns {
        if text.contains(pattern) {
            let target_date = today + chrono::Duration::days(*days_offset);
            if let Some(time) = NaiveTime::from_hms_opt(*hour, *minute, 0) {
                return Some(Utc.from_utc_datetime(&target_date.and_time(time)));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;

    /// 回归：截止时间必须取自"和待办相关的句子"，不能跨句乱借
    #[test]
    fn deadline_uses_relevant_sentence_only() {
        // 2026-10-28 是周三
        let now = Utc.with_ymd_and_hms(2026, 10, 28, 2, 0, 0).unwrap();

        // "月底前出一版方案" → 10-31，而不是同段里第一句的"今天"
        let conv = "今天站会老周说接口联调完了。另外客户要求月底前出一版方案。";
        let dt = resolve_deadline_in_text(conv, "出一版方案", now).unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2026-10-31");

        // "买维生素" 自己那句没有时间，不能借用上一句的"下周一"
        let conv2 = "下周一上午九点要去医院做年度体检，记得空腹。另外要买点维生素。";
        assert!(resolve_deadline_in_text(conv2, "买维生素", now).is_none());

        // 同句内的时间要能正确取到（本周五 = 10-30）
        let conv3 = "小陈负责写测试用例，本周五之前要给我。";
        let dt3 = resolve_deadline_in_text(conv3, "写测试用例", now).unwrap();
        assert_eq!(dt3.format("%Y-%m-%d").to_string(), "2026-10-30");
    }

    #[test]
    fn month_edge_is_parsed() {
        let now = Utc.with_ymd_and_hms(2026, 10, 28, 2, 0, 0).unwrap();
        assert_eq!(parse_event_time("月底", now).unwrap().format("%Y-%m-%d").to_string(), "2026-10-31");
        assert_eq!(parse_event_time("月初", now).unwrap().format("%Y-%m-%d").to_string(), "2026-10-01");
        // 12 月的月底要跨年
        let dec = Utc.with_ymd_and_hms(2026, 12, 10, 2, 0, 0).unwrap();
        assert_eq!(parse_event_time("月底", dec).unwrap().format("%Y-%m-%d").to_string(), "2026-12-31");
    }

    fn create_reference_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 2, 10, 0, 0).unwrap()
    }

    #[test]
    fn test_parse_absolute_time_with_date_and_hour() {
        let ref_time = create_reference_time();
        let result = parse_event_time("2026-06-03 15:00", ref_time);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 3);
        assert_eq!(dt.hour(), 15);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_parse_absolute_time_with_date() {
        let ref_time = create_reference_time();
        let result = parse_event_time("2026-06-10", ref_time);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 10);
        assert_eq!(dt.hour(), 0);
    }

    #[test]
    fn test_parse_today_with_time() {
        let ref_time = create_reference_time();
        let result = parse_event_time("今天下午2点", ref_time);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 2);
        assert_eq!(dt.hour(), 14);
    }

    #[test]
    fn test_parse_tomorrow_with_time() {
        let ref_time = create_reference_time();
        let result = parse_event_time("明天上午10点", ref_time);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 3);
        assert_eq!(dt.hour(), 10);
    }

    #[test]
    fn test_parse_weekday() {
        let ref_time = create_reference_time();
        let result = parse_event_time("下周三", ref_time);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.weekday(), Weekday::Wed);
    }

    #[test]
    fn test_parse_weekday_with_afternoon() {
        let ref_time = create_reference_time();
        let result = parse_event_time("下周三下午", ref_time);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.weekday(), Weekday::Wed);
        assert_eq!(dt.hour(), 14);
    }

    #[test]
    fn test_parse_days_later() {
        let ref_time = create_reference_time();
        let result = parse_event_time("3天后", ref_time);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.day(), 5);
    }

    #[test]
    fn test_parse_chinese_date() {
        let ref_time = create_reference_time();
        let result = parse_event_time("6月15日", ref_time);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 15);
    }

    #[test]
    fn test_parse_with_colon_time() {
        let ref_time = create_reference_time();
        let result = parse_event_time("今天 15:30", ref_time);
        assert!(result.is_some());
        let dt = result.unwrap();
        assert_eq!(dt.hour(), 15);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_parse_invalid() {
        let ref_time = create_reference_time();
        let result = parse_event_time("无所谓的时间", ref_time);
        assert!(result.is_none());
    }
}
/// 从对话原文里解析与某个待办相关的截止时间
///
/// 背景：小模型（3B）给出的 ISO 日期经常漂移甚至给出过去的时间
/// （例："下周一之前要完成" → 2026-07-11）。这里以**对话原文**为准，
/// 优先选用与待办标题重合度最高的句子，再用 [`parse_event_time`] 解析其中的
/// 相对时间表达，结果比依赖模型更可靠。
///
/// 解析结果统一归一到当天 23:59:59（与项目内待办 deadline 约定一致）。
pub fn resolve_deadline_in_text(
    conversation: &str,
    title: &str,
    reference_time: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let sentences: Vec<&str> = conversation
        .split(|c| c == '。' || c == '！' || c == '？' || c == ';' || c == '；' || c == '\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if sentences.is_empty() {
        return None;
    }

    let title_chars: Vec<char> = title.chars().filter(|c| !c.is_whitespace()).collect();
    let title_len = title_chars.len().max(1);

    // 1) 找出"确实在说这件事"的句子：用**覆盖度**而不是单字命中
    //
    // 原实现只统计"标题里有多少个字出现在句子中"，中文常用字（一、的、要…）
    // 几乎人人都命中，导致随便挑中第一句，于是把无关句子的时间当成截止时间
    // （实测："出一版方案" 被安上了"今天"）。
    let mut best: Option<(f32, usize, &str)> = None;
    for (idx, sentence) in sentences.iter().enumerate() {
        let hit = title_chars.iter().filter(|c| sentence.contains(**c)).count();
        let ratio = hit as f32 / title_len as f32;
        if hit >= 2 && ratio >= 0.5 {
            match best {
                Some((score, _, _)) if score >= ratio => {}
                _ => best = Some((ratio, idx, sentence)),
            }
        }
    }

    // 2) 只在这句话里找时间；找不到才看**紧邻的下一句**
    //
    // 不跨整段乱找：宁可返回 None（调用方用默认 +7 天），也不要给一个错误日期。
    // 允许看下一句是为了覆盖"…，下周五之前要给我。"这种被逗号/句号拆开的情况。
    // 情况 A：有匹配句 → 只在这句（和紧邻下一句）里找，绝不跨句借用，
    //         因为"匹配句里没写时间"时应回落到默认截止日，而不是拿别的句子的时间凑数。
    // 情况 B：整段都跟标题不相关（如标题是外部生成的短语）→ 退回按顺序扫描，
    //         这是历史行为，保持兼容（见 test_resolve_deadline_fallback_to_first_sentence）。
    let candidates: Vec<&str> = match best {
        Some((_, idx, sentence)) => {
            let mut v = vec![sentence];
            if let Some(next) = sentences.get(idx + 1) {
                v.push(next);
            }
            v
        }
        None => sentences.clone(),
    };

    for sentence in candidates {
        // 同一句里可能有多处时间，逐个尝试（取第一个能解析的）
        for expr in find_time_exprs(sentence) {
            if let Some(dt) = parse_event_time(&expr, reference_time) {
                return Some(end_of_day(dt));
            }
        }
    }

    None
}

/// 找出句中所有"精确时间表达"（按出现顺序）
fn find_time_exprs(sentence: &str) -> Vec<String> {
    let re = match regex::Regex::new(
        r"(大后天|下下周[一二三四五六日]|下周[一二三四五六日]|本周[一二三四五六日]|这周[一二三四五六日]|周[一二三四五六日]|下下周|下周|本周|这周|今天|今日|明天|明日|后天|昨天|前天|周末|月底|月初|\d{1,2}月\d{1,2}日|\d{1,2}号)",
    ) {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };
    re.find_iter(sentence).map(|m| m.as_str().to_string()).collect()
}

/// 找出句中的"精确时间表达"
///
/// 只认能唯一确定某一天的表达：今天/明天/后天/周X/下周X/某月某日/某号。
/// 明确排除"下季度""下个月""今年"这类**时间范围**——
/// 例如"整理下季度的客户反馈，下周一之前要完成"里，
/// 截止日是"下周一"而不是"下季度"（后者会让日期偏到季度初）。
fn find_precise_time_expr(sentence: &str) -> Option<String> {
    let re = regex::Regex::new(
        r"(大后天|下下周[一二三四五六日]|下周[一二三四五六日]|本周[一二三四五六日]|这周[一二三四五六日]|周[一二三四五六日]|下下周|下周|本周|这周|上个月|下个月|今天|今日|明天|明日|后天|昨天|前天|周末|月底|月初|\d{1,2}月\d{1,2}日|\d{1,2}号)",
    )
    .ok()?;
    re.find(sentence).map(|m| m.as_str().to_string())
}

/// 归一到当天 23:59:59
fn end_of_day(dt: DateTime<Utc>) -> DateTime<Utc> {
    let date = dt.date_naive();
    DateTime::<Utc>::from_naive_utc_and_offset(
        date.and_hms_opt(23, 59, 59).unwrap_or_else(|| date.and_hms_opt(0, 0, 0).unwrap()),
        Utc,
    )
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    use chrono::TimeZone;

    fn ref_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 28, 10, 0, 0).unwrap()
    }

    #[test]
    fn test_resolve_deadline_prefers_matching_sentence() {
        let conversation = "下周三我们需要交付第二阶段的接口文档，deadline 定在下周五之前。\
             另外我答应李姐帮她整理下季度的客户反馈，下周一之前要完成。";
        let dt = resolve_deadline_in_text(conversation, "整理下季度客户反馈", ref_time()).unwrap();
        // 2026-10-28 是周三，"下周一" = 2026-11-02
        assert_eq!(dt.date_naive().to_string(), "2026-11-02");
        assert_eq!(dt.time(), chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap());
    }

    #[test]
    fn test_resolve_deadline_fallback_to_first_sentence() {
        let conversation = "明天下午三点开会。顺便把周报写了。";
        let dt = resolve_deadline_in_text(conversation, "不相关的标题", ref_time()).unwrap();
        assert_eq!(dt.date_naive().to_string(), "2026-10-29");
    }

    #[test]
    fn test_resolve_deadline_none_when_no_time_expression() {
        assert!(resolve_deadline_in_text("随便聊聊，没有任何时间。", "随便", ref_time()).is_none());
    }
}
