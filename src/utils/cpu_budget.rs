//! 进程级 CPU 预算
//!
//! 设计约束是「**本项目**占整机 CPU 不超过 N%」，不是「线程数不超过核心数的 N%」。
//! 两者不等价：线程数只是上限的间接代理，API/嵌入/心跳等线程并未被计入，
//! 而且 CPU 模式下原先根本没有约束（只有 GPU 模式压了 llama.cpp 线程数）。
//!
//! 这里用两条独立的闸门：
//! 1. **作业对象 CPU 速率硬上限**（Windows）：由内核强制整个进程的 CPU 时间片占比，
//!    覆盖所有线程（推理、嵌入、tokio worker、心跳），是唯一能真正封顶的手段；
//! 2. **线程预算**：把 llama.cpp / ONNX 的线程数压到 `核心数 × N%`，降低争抢，
//!    也让"平台不支持硬上限"时有兜底。

use std::sync::OnceLock;

static APPLIED: OnceLock<Result<u32, String>> = OnceLock::new();

/// 给当前进程施加 CPU 硬上限（占整机百分比）。
///
/// 重复调用只会生效一次，返回首次结果。
pub fn apply_process_cpu_cap(percent: u32) -> Result<u32, String> {
    let pct = percent.clamp(1, 100);
    APPLIED.get_or_init(|| imp::apply(pct)).clone()
}

/// 线程预算：**整机逻辑核数** × 百分比（向下取整，至少 1）
///
/// 语义就是"本项目允许占用整机 CPU 的 N%"：32 逻辑核 × 60% → 19 个线程，
/// 也就是允许吃到整机 60%（16 物理核会跑超线程，这是操作系统的事）。
///
/// 历史问题：
/// - 旧实现 `物理核 × 百分比`：16 × 60% → 9 线程，整机只能吃到 28%；
/// - 中间版本又用物理核封顶（16 线程 → 50%），仍然低于设计上限。
/// 两者都让"上限"变成了"天花板"，用户看到的是"CPU 只用 30%，模型像没在推理"。
pub fn thread_budget(logical: usize, percent: u32) -> usize {
    let pct = percent.clamp(1, 100) as usize;
    let logical = logical.max(1);
    ((logical * pct) / 100).max(1)
}

/// 返回 (物理核数, 逻辑核数)
pub fn core_counts() -> (usize, usize) {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1);
    let physical = num_cpus::get_physical().max(1);
    (physical.min(logical), logical)
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::OnceLock;

    type Handle = *mut c_void;

    /// JOBOBJECT_CPU_RATE_CONTROL_INFORMATION
    ///
    /// 实测这台 Windows 只接受 8 字节（ControlFlags + CpuRate）；
    /// 传 12 字节（多带 MinRate/MaxRate）会返回 ERROR_BAD_LENGTH(24)。
    #[repr(C)]
    struct CpuRateInfo {
        control_flags: u32,
        cpu_rate: u32,
    }

    // JOBOBJECT_CPU_RATE_CONTROL_INFORMATION
    const JOB_OBJECT_CPU_RATE_CONTROL_ENABLE: u32 = 0x1;
    const JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP: u32 = 0x4;
    const JOB_OBJECT_CPU_RATE_CONTROL_INFORMATION_CLASS: i32 = 15;

    extern "system" {
        fn CreateJobObjectW(attrs: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn GetCurrentProcess() -> Handle;
        fn GetLastError() -> u32;
    }

    // 句柄必须活到进程结束：作业对象被关闭时限制会解除。
    static JOB: OnceLock<usize> = OnceLock::new();

    pub fn apply(percent: u32) -> Result<u32, String> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if job.is_null() {
                return Err(format!("CreateJobObjectW 失败: {}", GetLastError()));
            }

            let info = CpuRateInfo {
                control_flags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                    | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
                // CpuRate 单位是 1/100 个百分点：6000 = 60%
                cpu_rate: percent * 100,
            };

            if SetInformationJobObject(
                job,
                JOB_OBJECT_CPU_RATE_CONTROL_INFORMATION_CLASS,
                &info as *const _ as *const c_void,
                std::mem::size_of::<CpuRateInfo>() as u32,
            ) == 0
            {
                return Err(format!("SetInformationJobObject 失败: {}", GetLastError()));
            }

            if AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
                // 常见原因：进程已在另一个不允许嵌套的作业里，或权限不足
                return Err(format!("AssignProcessToJobObject 失败: {}", GetLastError()));
            }

            let _ = JOB.set(job as usize);
            Ok(percent)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn apply(_percent: u32) -> Result<u32, String> {
        Err("当前平台不支持进程级 CPU 硬上限，将退化为线程预算".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 预算 = 逻辑核 × 百分比（就是"整机占比"本身）
    ///
    /// 32 逻辑核、60% → 19 线程，允许吃到整机 60%。
    #[test]
    fn test_thread_budget_is_logical_times_percent() {
        assert_eq!(thread_budget(32, 60), 19);
        assert_eq!(thread_budget(32, 50), 16);
        assert_eq!(thread_budget(16, 60), 9);
        assert_eq!(thread_budget(8, 100), 8);
        // 小核心数也不能算出 0
        assert_eq!(thread_budget(1, 60), 1);
    }

    #[test]
    fn test_thread_budget_clamps_weird_percentages() {
        assert_eq!(thread_budget(32, 0), 1); // 0% 视为 1%
        assert_eq!(thread_budget(32, 250), 32); // >100% 视为 100%
    }
}
