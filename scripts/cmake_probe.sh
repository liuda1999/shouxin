#!/usr/bin/env bash
# 手动执行 llama-cpp-sys 的 cmake configure，用于定位构建期问题
# （cargo 会把 cmake 的报错藏在 build script 输出里，这个脚本能直接看到）
#
# 用法：
#   bash scripts/cmake_probe.sh                # 自动定位 crate 与构建目录
#   bash scripts/cmake_probe.sh <build_dir>    # 指定构建目录
#   SHOUXIN_CUDA=1 bash scripts/cmake_probe.sh # 按 CUDA 后端探测
#
# 工具链路径全部来自 scripts/dev_env.sh 的自动探测，本脚本不写死任何机器路径。
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# shellcheck source=./dev_env.sh
source "$ROOT/scripts/dev_env.sh"

# 定位 llama-cpp-sys 源码（版本从 Cargo.lock 读，避免写死）
SYS_VER="$(grep -A 1 '^name = "llama-cpp-sys-2"' Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p' | head -1)"
if [ -z "$SYS_VER" ]; then
  echo "未在 Cargo.lock 中找到 llama-cpp-sys-2 版本" >&2
  exit 1
fi
CRATE="$(ls -d "$HOME"/.cargo/registry/src/*/"llama-cpp-sys-2-$SYS_VER" 2>/dev/null | head -1)"
if [ -z "$CRATE" ]; then
  echo "未找到 crate 源码目录：~/.cargo/registry/src/*/llama-cpp-sys-2-$SYS_VER" >&2
  echo "先执行一次 cargo build --features gguf 让 cargo 下载依赖" >&2
  exit 1
fi

BUILD_DIR="${1:-$ROOT/target/debug/build/llama-cpp-sys-2-probe/out/build}"
mkdir -p "$BUILD_DIR"

CUDA_FLAG=OFF
[ "${SHOUXIN_CUDA:-0}" = "1" ] && CUDA_FLAG=ON

echo "==> crate:  $CRATE"
echo "==> build:  $BUILD_DIR"
echo "==> cuda:   $CUDA_FLAG"

"$CMAKE" "$CRATE/llama.cpp" -B "$BUILD_DIR" -G Ninja \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=OFF \
  -DLLAMA_BUILD_TOOLS=OFF -DLLAMA_BUILD_COMMON=ON -DLLAMA_CURL=OFF \
  -DCMAKE_BUILD_PARALLEL_LEVEL="${SHOUXIN_JOBS:-16}" -DGGML_NATIVE=OFF \
  -DBUILD_SHARED_LIBS=OFF -DGGML_CUDA="$CUDA_FLAG" -DGGML_OPENMP=OFF \
  -DCMAKE_INSTALL_PREFIX="$BUILD_DIR/../" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_COMPILER="$CMAKE_C_COMPILER" \
  -DCMAKE_CXX_COMPILER="$CMAKE_CXX_COMPILER" 2>&1 | tail -30
