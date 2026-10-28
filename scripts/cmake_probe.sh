#!/usr/bin/env bash
# 手动执行 llama-cpp-sys 的 cmake configure，以便看到 cmake 自身的报错
set -uo pipefail

MSVC_ROOT="C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.44.35207"
SDK_ROOT="C:\\Program Files (x86)\\Windows Kits\\10"
SDK_VER="10.0.26100.0"
NINJA_DIR="/d/2026-ComfyUI-V8.3/python/Scripts"
CRATE="$(ls -d "$HOME"/.cargo/registry/src/*/llama-cpp-sys-2-0.1.146 | head -1)"

export INCLUDE="${MSVC_ROOT}\\include;${SDK_ROOT}\\Include\\${SDK_VER}\\ucrt;${SDK_ROOT}\\Include\\${SDK_VER}\\shared;${SDK_ROOT}\\Include\\${SDK_VER}\\um"
export LIB="${MSVC_ROOT}\\lib\\x64;${SDK_ROOT}\\Lib\\${SDK_VER}\\ucrt\\x64;${SDK_ROOT}\\Lib\\${SDK_VER}\\um\\x64"
export PATH="/c/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64:/c/Program Files (x86)/Windows Kits/10/bin/${SDK_VER}/x64:${NINJA_DIR}:/c/Program Files/NVIDIA GPU Computing Toolkit/CUDA/v12.8/bin:$PATH"
export CMAKE_MAKE_PROGRAM="${NINJA_DIR}/ninja.exe"

BUILD_DIR="${1:-E:/ShouXin/target/debug/build/llama-cpp-sys-2-d11ce301c1a89acc/out/build}"

"/d/2026-ComfyUI-V8.3/python/Lib/site-packages/cmake/data/bin/cmake.exe" \
  "$CRATE/llama.cpp" -B "$BUILD_DIR" -G Ninja \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=OFF \
  -DLLAMA_BUILD_TOOLS=OFF -DLLAMA_BUILD_COMMON=ON -DLLAMA_CURL=OFF \
  -DCMAKE_BUILD_PARALLEL_LEVEL=32 -DGGML_NATIVE=OFF -DBUILD_SHARED_LIBS=OFF \
  -DGGML_CUDA=ON -DGGML_OPENMP=OFF \
  -DCMAKE_INSTALL_PREFIX="$BUILD_DIR/../" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_COMPILER="C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/cl.exe" \
  -DCMAKE_CXX_COMPILER="C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207/bin/Hostx64/x64/cl.exe" 2>&1 | tail -30
