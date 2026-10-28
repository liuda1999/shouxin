#!/usr/bin/env bash
# 开发环境变量（Windows + MSVC 专用）
#
# 作用：把编译 llama.cpp 需要的 MSVC/SDK/cmake/ninja 环境导出到当前 shell，
# 这样任意 cargo 命令都能跑，而不仅限于 scripts/build_gguf.sh。
#
# 用法：
#   source scripts/dev_env.sh
#   cargo test --features gguf --lib
#   cargo build --release --features gguf
#
# 背景：本机 cmd.exe 不可用（拿不到 vcvars 环境），cmake 用 Ninja 生成器时需要
#       INCLUDE / LIB / PATH 手工拼好；且 Windows 版 cmake/ninja 必须用绝对路径，
#       否则 cargo 构建脚本内部以 POSIX 形态 PATH 调用它们会「静默退出 1」。

MSVC_ROOT="C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.44.35207"
SDK_ROOT="C:\\Program Files (x86)\\Windows Kits\\10"
SDK_VER="10.0.26100.0"
NINJA_DIR="/d/2026-ComfyUI-V8.3/python/Scripts"
CMAKE_BIN_DIR="/d/2026-ComfyUI-V8.3/python/Lib/site-packages/cmake/data/bin"

export INCLUDE="${MSVC_ROOT}\\include;${SDK_ROOT}\\Include\\${SDK_VER}\\ucrt;${SDK_ROOT}\\Include\\${SDK_VER}\\shared;${SDK_ROOT}\\Include\\${SDK_VER}\\um;${SDK_ROOT}\\Include\\${SDK_VER}\\winrt"
export LIB="${MSVC_ROOT}\\lib\\x64;${SDK_ROOT}\\Lib\\${SDK_VER}\\ucrt\\x64;${SDK_ROOT}\\Lib\\${SDK_VER}\\um\\x64"
export LIBPATH="$LIB"
export PATH="/c/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64:/c/Program Files (x86)/Windows Kits/10/bin/${SDK_VER}/x64:${NINJA_DIR}:${CMAKE_BIN_DIR}:$PATH"

export CMAKE_GENERATOR=Ninja
export CMAKE="D:\\2026-ComfyUI-V8.3\\python\\Lib\\site-packages\\cmake\\data\\bin\\cmake.exe"
export CMAKE_MAKE_PROGRAM="D:\\2026-ComfyUI-V8.3\\python\\Scripts\\ninja.exe"
export CMAKE_C_COMPILER="C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/cl.exe"
export CMAKE_CXX_COMPILER="$CMAKE_C_COMPILER"
export CC="$CMAKE_C_COMPILER"
export CXX="$CMAKE_C_COMPILER"

# 必须留空 MSYS 的参数转换，否则 /utf-8 会被当成路径改写
export MSYS2_ARG_CONV_EXCL="*"
export CMAKE_TOOLCHAIN_FILE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/llama_cpu_toolchain.cmake"
export RUSTFLAGS="-C target-cpu=native"

export CFLAGS="-utf-8"
export CXXFLAGS="-utf-8"
export CFLAGS_x86_64_pc_windows_msvc="-utf-8"
export CXXFLAGS_x86_64_pc_windows_msvc="-utf-8"

export PATH="$HOME/.cargo/bin:$PATH"

echo "[dev_env] 环境已就绪：MSVC + Ninja + AVX2 工具链"
