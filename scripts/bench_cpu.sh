#!/usr/bin/env bash
# CPU 推理基准：编译并运行 bench_gguf，做线程数扫描
#
# 说明：早期版本这里尝试用仓库自带的 llama.cpp 源码单独编译 llama-bench，
# 但 llama-cpp-sys 的 crate 包缺少 llama.cpp/app 等子目录，独立 configure 会失败
# （add_subdirectory 找不到目录）。因此改为使用项目内的 bench_gguf：
# 它复用同一套 llama.cpp 静态库和推理参数，口径与线上一致。
#
# 用法： bash scripts/bench_cpu.sh [模型路径]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODEL="${1:-$(grep -E '^model_path' config.toml | head -1 | sed 's/.*= *"//;s/"$//')}"
MODEL="${MODEL:-./models/model.gguf}"

if [ ! -f "$MODEL" ]; then
  echo "找不到模型: $MODEL"
  echo "用法: bash scripts/bench_cpu.sh <模型路径>"
  exit 1
fi

echo "==> 编译 bench_gguf"
SHOUXIN_BIN=bench_gguf bash scripts/build_gguf.sh >/dev/null

BIN="./target/debug/bench_gguf.exe"
[ -f "$BIN" ] || BIN="./target/debug/bench_gguf"

echo "==> 模型: $MODEL"
echo ""
echo "线程扫描（每次跑 16 与 128 token 两轮，用差值算纯生成速度）"
printf "%-10s %-12s %-12s %-12s\n" "threads" "16tok(s)" "128tok(s)" "生成tok/s"

for T in 4 8 16 32; do
  T16=$("$BIN" --model "$MODEL" --threads "$T" --max-tokens 16  --runs 1 2>/dev/null | grep -oE '\| [0-9.]+s' | head -1 | tr -dc '0-9.')
  T128=$("$BIN" --model "$MODEL" --threads "$T" --max-tokens 128 --runs 1 2>/dev/null | grep -oE '\| [0-9.]+s' | head -1 | tr -dc '0-9.')
  if [ -n "$T16" ] && [ -n "$T128" ]; then
    TPS=$(awk -v a="$T16" -v b="$T128" 'BEGIN{d=(b-a)/112; if(d>0) printf "%.1f", 1/d; else print "-"}')
  else
    TPS="-"
  fi
  printf "%-10s %-12s %-12s %-12s\n" "$T" "${T16:-fail}" "${T128:-fail}" "$TPS"
done

echo ""
echo "参考：Ryzen 9 9950X / Ministral-3B Q8_0 / 16 线程 ≈ 15 tok/s；Gemma-E4B ≈ 16 tok/s；"
echo "低于 5 tok/s 说明指令集没生效，见 docs/PERFORMANCE.md"
