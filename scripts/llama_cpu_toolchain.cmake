# 为 llama.cpp 强制开启 x86 SIMD 指令集（MSVC 专用补丁）
#
# 背景（实测证据）：
#   llama-cpp-sys 在 Rust 侧未用 `-C target-cpu=native` 时会把 `GGML_NATIVE=OFF`；
#   即使用 `target-cpu=native` 打开 `GGML_NATIVE=ON`，其 CMake 依赖 `-march=native`
#   （GCC/Clang 语义），**MSVC 不认识该参数**，指令集探测静默失败。
#
#   更关键的一步：只设置 `GGML_AVX2=ON` 这类缓存变量**并不会**让 MSVC 收到
#   `/arch:AVX2` —— 实测 build.ninja 里 408 个编译单元的 FLAGS 全都没有任何
#   `/arch:` 参数，ggml-cpu 因此退化到 SSE2 基线内核，
#   CPU 推理只有 ~1 tok/s（正确应为 20+ tok/s）。
#
# 因此这里除了打开 ggml 的开关，**必须**把 `/arch:AVX2` 显式塞进
# CMAKE_C_FLAGS / CMAKE_CXX_FLAGS，让 `__AVX2__` 宏真正被定义，
# ggml-cpu 里的 AVX2/FMA 内核才会参与编译。
#
# 适用：支持 AVX2 的 x86_64 CPU（Intel Haswell+ / AMD Zen+）。
# 更老的机器请把 AVX2 换成 /arch:AVX，或删除本文件回退标量构建。

# === 关键补丁 1：告诉 CMake 目标架构 ===
#
# 本机构建环境里 `CMAKE_SYSTEM_PROCESSOR` 是**空值**（Ninja + 显式 cl.exe、
# 没有 vcvars 环境时常见），后果非常隐蔽：
#   ggml 的 CMakeLists 会走到最后的兜底分支
#       message(WARNING "Unknown CPU architecture. Falling back to generic implementations.")
#       list(APPEND ARCH_FLAGS -DGGML_CPU_GENERIC)
#   于是 ggml-cpu 只编译**通用标量实现**，哪怕命令行里有 /arch:AVX2 也没用
#   （`__AVX2__` 有定义，但代码路径被 GGML_CPU_GENERIC 挡掉了）。
#   实测：3B Q5_K_M 只有 ~8 tok/s（同机 LM Studio 有 20 tok/s）。
#
# 必须在 project() 之前（也就是工具链文件里）设好，后面再改无效。
if(NOT CMAKE_SYSTEM_PROCESSOR)
    set(CMAKE_SYSTEM_PROCESSOR "AMD64")
endif()
if(NOT CMAKE_HOST_SYSTEM_PROCESSOR)
    set(CMAKE_HOST_SYSTEM_PROCESSOR "AMD64")
endif()

set(GGML_AVX   ON CACHE BOOL "ggml: enable AVX"  FORCE)
set(GGML_AVX2  ON CACHE BOOL "ggml: enable AVX2" FORCE)
set(GGML_FMA   ON CACHE BOOL "ggml: enable FMA"  FORCE)
set(GGML_F16C  ON CACHE BOOL "ggml: enable F16C" FORCE)
set(GGML_BMI2  ON CACHE BOOL "ggml: enable BMI2" FORCE)

# AVX512：Zen4/Zen5 与 Intel Skylake-X 之后支持。默认关闭以便兼容老 CPU；
# 想要极致性能可设为 ON 并把下面的 /arch:AVX2 改为 /arch:AVX512。
set(GGML_AVX512      OFF CACHE BOOL "ggml: enable AVX512F"     FORCE)
set(GGML_AVX512_VBMI OFF CACHE BOOL "ggml: enable AVX512-VBMI" FORCE)
set(GGML_AVX512_VNNI OFF CACHE BOOL "ggml: enable AVX512-VNNI" FORCE)

# === 关键补丁：真正把指令集交给编译器 ===
set(_SHOUXIN_SIMD_FLAGS "/arch:AVX2 /utf-8")

string(APPEND CMAKE_C_FLAGS   " ${_SHOUXIN_SIMD_FLAGS}")
string(APPEND CMAKE_CXX_FLAGS " ${_SHOUXIN_SIMD_FLAGS}")
set(CMAKE_C_FLAGS   "${CMAKE_C_FLAGS}"   CACHE STRING "C flags (SIMD patched)"   FORCE)
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}" CACHE STRING "CXX flags (SIMD patched)" FORCE)

message(STATUS "[shouxin] llama.cpp SIMD flags: ${_SHOUXIN_SIMD_FLAGS}")
