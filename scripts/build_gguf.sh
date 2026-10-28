#!/usr/bin/env bash
# 编译带 llama.cpp(GGUF) 后端的服务二进制
#
# 为什么需要这个脚本：
#  1. llama-cpp-sys 由 cmake 驱动；cmake 的 "Visual Studio" 生成器需要读注册表，
#     受限环境常禁止 reg.exe 调用 → 统一改用 Ninja 生成器（不查注册表）。
#  2. 没有 vcvars 环境时，需要手工拼 MSVC/SDK 的 INCLUDE / LIB / PATH。
#  3. llama.cpp 源码里有 UTF-8 字符（如 "↵"），MSVC 默认按 GBK 解析会报
#     "C2001: newline in constant"，需先把该字符替换为 ASCII（见 fix_llama_cpp_utf8.sh）。
#
# 环境探测全部交给 scripts/dev_env.sh（不写死任何机器路径），本脚本只负责编排。
#
# 用法：
#   bash scripts/build_gguf.sh                    # debug 构建 shouxin-server
#   bash scripts/build_gguf.sh --release          # release 构建
#   SHOUXIN_BIN=bench_gguf bash scripts/build_gguf.sh
#   SHOUXIN_FEATURES=gguf-cuda bash scripts/build_gguf.sh   # 需要 CUDA Toolkit
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# 导入自动探测出来的工具链环境
# shellcheck source=./dev_env.sh
source "$ROOT/scripts/dev_env.sh"

echo "==> 预处理 llama.cpp 源码编码（MSVC + GBK 代码页会报错）"
bash scripts/fix_llama_cpp_utf8.sh

BIN="${SHOUXIN_BIN:-shouxin-server}"
FEATURES="${SHOUXIN_FEATURES:-gguf}"

echo "==> 编译目标: $BIN（features=$FEATURES，首次需编译 llama.cpp，耗时较长）"
cargo build --features "$FEATURES" --bin "$BIN" "$@"

# 输出真实产物路径（debug/release、是否带 .exe 都自动适配）
OUT="$ROOT/target/debug/$BIN"
[ -f "$OUT" ] || OUT="$ROOT/target/debug/$BIN.exe"
case " $* " in *" --release "*) OUT="$ROOT/target/release/$BIN"; [ -f "$OUT" ] || OUT="$ROOT/target/release/$BIN.exe";; esac
echo "==> 完成：$OUT"
