# ShouXin v0.5.1 - 修复问题3：事件时间结构化（E-5）

## 一、问题描述

当前系统提取事件时，`time_info` 字段存储的是对话创建时间，而非事件实际发生时间（如"明天下午2点"）。需引入专用时间解析模块，将自然语言时间转换为标准 ISO 时间戳。

## 二、解决方案

使用规则 + 正则 + 轻量库（`chrono` + `regex`）解析自然语言时间，不依赖 LLM。

## 三、核心约束

- 不得依赖 LLM 解析时间（因其不稳定）
- 必须使用规则 + 正则 + 轻量库
- 不得移除原有 `time_info` 字段，但应新增 `event_time` 字段
- 不得省略时区处理

## 四、实现详情

### 4.1 时间解析模块

创建 `src/utils/time_parser.rs`，实现函数：

```rust
pub fn parse_event_time(text: &str, reference_time: DateTime<Utc>) -> Option<DateTime<Utc>>
```

### 4.2 支持的时间表达

| 类型 | 示例 | 解析结果 |
|------|------|----------|
| 绝对时间 | `2026-06-03 15:00` | 2026-06-03T15:00:00Z |
| 绝对时间 | `2026-06-10` | 2026-06-10T00:00:00Z |
| 相对时间 | `今天下午3点` | 今天 15:00:00 |
| 相对时间 | `明天上午10点` | 明天 10:00:00 |
| 相对时间 | `3天后` | 3天后的 09:00:00 |
| 星期 | `下周三` | 下周三 10:00:00 |
| 星期+时间 | `下周三下午` | 下周三 14:00:00 |
| 月/日 | `6月15日` | 当年6月15日 00:00:00 |
| 模糊时间 | `今晚` | 当天晚上 20:00:00 |

### 4.3 解析策略

1. **优先级顺序**：绝对时间 > 相对时间 > 星期 > 模糊时间
2. **正则提取**：优先使用正则提取数字和时间关键词
3. **时间段处理**：如"下午"映射到 14:00，"上午"映射到 09:00
4. **容错机制**：若无法解析，返回 `None`（不崩溃）

### 4.4 关键实现

```rust
// src/utils/time_parser.rs

pub fn parse_event_time(text: &str, reference_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
    // 1. 尝试解析绝对时间
    if let Some(dt) = parse_absolute_time(text, reference_time) {
        return Some(dt);
    }
    
    // 2. 尝试解析相对时间（今天、明天、N天后）
    if let Some(dt) = parse_relative_time(text, reference_time) {
        return Some(dt);
    }
    
    // 3. 尝试解析星期几
    if let Some(dt) = parse_weekday_time(text, reference_time) {
        return Some(dt);
    }
    
    // 4. 尝试解析模糊时间
    if let Some(dt) = parse_fuzzy_time(text, reference_time) {
        return Some(dt);
    }
    
    None
}

// 时间段映射
let time_patterns: HashMap<&str, (u32, u32)> = HashMap::from([
    ("早上", (8, 0)),
    ("上午", (9, 0)),
    ("中午", (12, 0)),
    ("下午", (14, 0)),
    ("傍晚", (18, 0)),
    ("晚上", (20, 0)),
]);

// 星期映射
let weekday_map: HashMap<&str, i64> = HashMap::from([
    ("周一", 0), ("星期一", 0),
    ("周二", 1), ("星期二", 1),
    ("周三", 2), ("星期三", 2),
    ("周四", 3), ("星期四", 3),
    ("周五", 4), ("星期五", 4),
    ("周六", 5), ("星期六", 5),
    ("周日", 6), ("星期日", 6),
]);
```

### 4.5 数据库扩展

```sql
ALTER TABLE event_index ADD COLUMN event_time TEXT;
```

### 4.6 EventIndex 结构体扩展

```rust
// src/core/contact.rs
pub struct EventIndex {
    // ... 原有字段 ...
    
    /// 事件实际发生时间（自然语言解析得到）
    pub event_time: Option<DateTime<Utc>>,
}
```

### 4.7 集成到事件处理流程

在 `ThreeLibraryMemoryManager` 中：

```rust
// src/memory/three_library_manager.rs

let event_parsed_time = parse_event_time(&e.description, Utc::now());

let event_index = crate::core::EventIndex {
    // ... 其他字段 ...
    event_time: event_parsed_time,
};
```

## 五、验证测试

### 5.1 单元测试结果

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_absolute_time_with_date_and_hour() {
        let result = parse_event_time("2026-06-03 15:00", ref_time);
        assert!(result.is_some());
        assert_eq!(result.unwrap().day(), 3);
        assert_eq!(result.unwrap().hour(), 15);
    }
    
    #[test]
    fn test_parse_today_with_time() {
        let result = parse_event_time("今天下午2点", ref_time);
        assert!(result.is_some());
        assert_eq!(result.unwrap().hour(), 14);
    }
    
    #[test]
    fn test_parse_tomorrow_with_time() {
        let result = parse_event_time("明天上午10点", ref_time);
        assert!(result.is_some());
        assert_eq!(result.unwrap().day(), 3); // 6月3日
        assert_eq!(result.unwrap().hour(), 10);
    }
    
    #[test]
    fn test_parse_weekday() {
        let result = parse_event_time("下周三", ref_time);
        assert!(result.is_some());
        assert_eq!(result.unwrap().weekday(), Weekday::Wed);
    }
    
    #[test]
    fn test_parse_invalid() {
        let result = parse_event_time("无所谓的时间", ref_time);
        assert!(result.is_none());
    }
}
```

### 5.2 集成测试用例

| 输入 | 预期输出 |
|------|----------|
| "今天下午2点短会" | `event_time` = 今天 14:00:00，`time_info` = "今天下午2点短会" |
| "明天上午10点评审" | `event_time` = 明天 10:00:00 |
| "下周三" | `event_time` = 下周三 10:00:00（默认时间） |
| "6月15日" | `event_time` = 当年6月15日 00:00:00 |

## 六、迁移脚本

```sql
-- migrations/v0_5_1_event_time.sql

ALTER TABLE event_index ADD COLUMN event_time TEXT;
```

## 七、注意事项

1. **保留原始字段**：`time_info` 保留原文本用于展示和调试
2. **时区处理**：默认使用 UTC，可通过配置调整
3. **解析失败**：返回 `None`，不崩溃，保持原字段不变
4. **默认时间**：若只指定日期而未指定时间，使用 10:00:00 作为默认时间
