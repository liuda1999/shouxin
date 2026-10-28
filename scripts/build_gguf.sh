#!/usr/bin/env bash
# 编译带 llama.cpp(GGUF) 后端的服务二进制
#
# 为什么需要这个脚本（本机环境的两个坑）：
#  1. llama-cpp-sys 由 cmake 驱动；cmake 的 "Visual Studio" 生成器需要读注册表，
#     而本机安全策略禁止调用 reg.exe → 改用 Ninja 生成器（不查注册表）。
#  2. 本机没有 vcvars 环境（cmd.exe 被沙箱拦截），因此这里手工拼 MSVC/SDK 的
#     INCLUDE / LIB / PATH。
#
# 另：llama.cpp 源码里有 UTF-8 字符（如 "↵"），MSVC 默认按 GBK 解析会报
#     "C2001: newline in constant"，需要预先把该字符替换为 ASCII。
#     见 scripts/fix_llama_cpp_utf8.sh
#
# 用法： bash scripts/build_gguf.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MSVC_ROOT="C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.44.35207"
SDK_ROOT="C:\\Program Files (x86)\\Windows Kits\\10"
SDK_VER="10.0.26100.0"
CUDA_ROOT="C:\\Program Files\\NVIDIA GPU Computing Toolkit\\CUDA\\v12.8"

# Ninja（ComfyUI 的 python 环境里自带，cmake 也可用它避开 VS 生成器）
NINJA_DIR="/d/2026-ComfyUI-V8.3/python/Scripts"
# 真正的 cmake 可执行文件在 pip 包的 data/bin 下（Scripts 下的那个是失效的 shim）
CMAKE_BIN_DIR="/d/2026-ComfyUI-V8.3/python/Lib/site-packages/cmake/data/bin"

export INCLUDE="${MSVC_ROOT}\\include;${SDK_ROOT}\\Include\\${SDK_VER}\\ucrt;${SDK_ROOT}\\Include\\${SDK_VER}\\shared;${SDK_ROOT}\\Include\\${SDK_VER}\\um;${SDK_ROOT}\\Include\\${SDK_VER}\\winrt"
export LIB="${MSVC_ROOT}\\lib\\x64;${SDK_ROOT}\\Lib\\${SDK_VER}\\ucrt\\x64;${SDK_ROOT}\\Lib\\${SDK_VER}\\um\\x64"
export LIBPATH="$LIB"
export PATH="/c/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64:/c/Program Files (x86)/Windows Kits/10/bin/${SDK_VER}/x64:${NINJA_DIR}:${CMAKE_BIN_DIR}:/c/Program Files/NVIDIA GPU Computing Toolkit/CUDA/v12.8/bin:$PATH"

export CMAKE_GENERATOR=Ninja
# cmake 与 ninja 都用 Windows 绝对路径指定：cargo 构建脚本内部再拉起 cmake/ninja 时，
# 依赖 POSIX 风格 PATH 会让它们找不到可执行文件（表现为 cmake 静默退出 1）。
export CMAKE="D:\\2026-ComfyUI-V8.3\\python\\Lib\\site-packages\\cmake\\data\\bin\\cmake.exe"
export CMAKE_MAKE_PROGRAM="D:\\2026-ComfyUI-V8.3\\python\\Scripts\\ninja.exe"
# 显式给出 cl.exe 绝对路径：cmake 用 Ninja 生成器时若靠探测定位 MSVC，
# 会去读注册表（reg.exe），本机安全策略禁止该调用 → 直接指定编译器可绕过。
export CMAKE_C_COMPILER="C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/cl.exe"
export CMAKE_CXX_COMPILER="$CMAKE_C_COMPILER"
export CC="$CMAKE_C_COMPILER"
export CXX="$CMAKE_C_COMPILER"
export CUDA_PATH="$CUDA_ROOT"

# 关键：用 target-cpu=native 触发 llama-cpp-sys 的 `GGML_NATIVE=ON`
# （build.rs 只有在检测到 target-cpu=native 时才开启，否则写死 GGML_NATIVE=OFF，
#   等于关掉 AVX2/AVX512 优化，CPU 推理会慢 20 倍以上：实测 1.2 tok/s → 期望 20+ tok/s）
#
# 注意：设置 RUSTFLAGS 环境变量会**覆盖** .cargo/config.toml 里的 rustflags，
# 所以这里必须把 CUDA 的链接路径一并带上。
export MSYS2_ARG_CONV_EXCL="*"
# MSVC 下必须显式开 AVX2，否则 ggml-cpu 退化为标量实现（慢 20 倍）
# MSVC 下必须显式开 AVX2，否则 ggml-cpu 退化为标量实现（慢 20 倍）
export CMAKE_TOOLCHAIN_FILE="E:/ShouXin/scripts/llama_cpu_toolchain.cmake"
export RUSTFLAGS="-C target-cpu=native"

# cc-rs 阶段（llama-cpp-sys 的 wrapper）同样会遇到 GBK 代码页问题：
# 强制 MSVC 按 UTF-8 解析源码
export CFLAGS="-utf-8"
export CXXFLAGS="-utf-8"
export CFLAGS_x86_64_pc_windows_msvc="-utf-8"
export CXXFLAGS_x86_64_pc_windows_msvc="-utf-8"

# cargo 可能不在默认 PATH（本机通过 rustup 安装）
export PATH="$HOME/.cargo/bin:$PATH"

echo "==> 预处理 llama.cpp 源码编码（MSVC + GBK 代码页会报错）"
bash scripts/fix_llama_cpp_utf8.sh

# 允许通过 SHOUXIN_BIN 指定编译目标（如 bench_gguf）
BIN="${SHOUXIN_BIN:-shouxin-server}"

echo "==> 编译目标: $BIN（首次需编译 llama.cpp，耗时较长）"
cargo build --features gguf --bin "$BIN" "$@"

echo "==> 完成：target/debug/shouxin-server.exe"
