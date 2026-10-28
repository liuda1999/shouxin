//! 工具模块
//!
//! 提供通用的验证和辅助函数

use crate::error::ShouxinError;

/// 验证字符串非空
#[inline(always)]
pub fn validate_non_empty(s: &str, field_name: &str) -> Result<(), ShouxinError> {
    if s.is_empty() {
        return Err(ShouxinError::InvalidArgument(format!(
            "{} cannot be empty",
            field_name
        )));
    }
    Ok(())
}

/// 验证浮点数是有效的（非 NaN、非 Inf）
#[inline(always)]
pub fn validate_finite(x: f32, field_name: &str) -> Result<(), ShouxinError> {
    if x.is_nan() || x.is_infinite() {
        return Err(ShouxinError::InvalidArgument(format!(
            "{} must be a valid finite number",
            field_name
        )));
    }
    Ok(())
}

/// 验证浮点数在指定范围内
#[inline(always)]
pub fn validate_range(x: f32, min: f32, max: f32, field_name: &str) -> Result<(), ShouxinError> {
    validate_finite(x, field_name)?;
    if x < min || x > max {
        return Err(ShouxinError::InvalidArgument(format!(
            "{} must be between {} and {}",
            field_name, min, max
        )));
    }
    Ok(())
}

/// 验证向量非空且所有元素有效
#[inline(always)]
pub fn validate_vector(v: &[f32], field_name: &str) -> Result<(), ShouxinError> {
    if v.is_empty() {
        return Err(ShouxinError::InvalidArgument(format!(
            "{} cannot be empty",
            field_name
        )));
    }
    if v.iter().any(|&x| x.is_nan() || x.is_infinite()) {
        return Err(ShouxinError::InvalidArgument(format!(
            "{} contains invalid values (NaN or infinite)",
            field_name
        )));
    }
    Ok(())
}
