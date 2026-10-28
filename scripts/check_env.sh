#!/usr/bin/env bash
# 环境自检：部署前跑一遍，缺什么直接给出修复命令
#
# 用法： bash scripts/check_env.sh
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# 支持校验非默认配置（如 config.3b.toml）：SHOUXIN_CONFIG=./config.3b.toml bash scripts/check_env.sh
CFG="${SHOUXIN_CONFIG:-config.toml}"

OK=0
WARN=0
FAIL=0

ok()   { echo "  [OK]   $*"; OK=$((OK+1)); }
warn() { echo "  [WARN] $*"; WARN=$((WARN+1)); }
fail() { echo "  [FAIL] $*"; FAIL=$((FAIL+1)); }

# 从 config.toml 的指定 section 里取值（兼容带引号与不带引号的写法）
cfg() { # cfg <section> <key>
  awk -v sec="$1" -v key="$2" '
    $0 ~ "^\\[" { cur=$0; next }
    cur=="[" sec "]" && $0 ~ "^[ \t]*" key "[ \t]*=" {
      sub(/^[^=]*=[ \t]*/, "");
      sub(/[ \t]*#.*$/, "");
      gsub(/"/, "");
      print; exit
    }
  ' "$CFG" 2>/dev/null
}

echo "== 1. 工具链 =="
CARGO_BIN=""
for c in cargo "$HOME/.cargo/bin/cargo" "$HOME/.cargo/bin/cargo.exe"; do
  if command -v "$c" >/dev/null 2>&1; then CARGO_BIN="$c"; break; fi
done
if [ -n "$CARGO_BIN" ]; then
  ok "cargo $("$CARGO_BIN" --version 2>&1 | awk '{print $2}')（$CARGO_BIN）"
else
  fail "未找到 cargo（安装：https://rustup.rs；或 cargo 不在 PATH，试试 \$HOME/.cargo/bin）"
fi

if command -v cmake >/dev/null 2>&1; then
  ok "cmake $(cmake --version 2>&1 | head -1 | awk '{print $3}')"
else
  warn "未找到 cmake —— 编译 gguf 特性时必需（Windows 推荐用 scripts/build_gguf.sh，它会自己找）"
fi

if command -v ninja >/dev/null 2>&1; then
  ok "ninja 可用"
else
  warn "未找到 ninja（可用 Makefile，但推荐 ninja）"
fi

echo "== 2. 推理模型（GGUF）=="
MODEL=$(cfg "llm.gguf" "model_path")
MODEL="${MODEL:-./models/model.gguf}"
if [ -f "$MODEL" ]; then
  SIZE=$(stat -c %s "$MODEL" 2>/dev/null || stat -f %z "$MODEL" 2>/dev/null)
  HEAD4=$(head -c 4 "$MODEL" 2>/dev/null)
  if [ "$HEAD4" = "GGUF" ]; then
    ok "$MODEL（$((SIZE/1048576)) MB，GGUF 头正确）"
  else
    fail "$MODEL 头部不是 GGUF（读到 '$HEAD4'）—— 文件可能损坏/被截断，请重新下载"
  fi
else
  fail "缺少推理模型：$MODEL"
  echo "         下载见 docs/MODEL_SETUP.md，例如："
  echo "         python scripts/download_gguf.py \"<hf-url>\" \"$MODEL\" 32"
fi

echo "== 3. 嵌入模型（ONNX）=="
CACHE=$(cfg "embedder" "model_path")
CACHE="${CACHE:-.fastembed_cache}"
EMODEL=$(cfg "embedder" "model")
EMODEL="${EMODEL:-intfloat/multilingual-e5-base}"
BACKEND=$(cfg "embedder" "backend")
ONNX="$CACHE/$EMODEL/onnx/model.onnx"
if [ -f "$ONNX" ]; then
  ok "$ONNX（$(( $(stat -c %s "$ONNX" 2>/dev/null || stat -f %z "$ONNX")/1048576 )) MB）"
else
  if [ "$BACKEND" = "hash" ]; then
    ok "未使用 ONNX（backend = hash，离线哈希嵌入）"
  else
    warn "缺少嵌入模型：$ONNX"
    echo "         下载： bash scripts/fetch_embedding_model.sh"
    echo "         若 fallback_to_hash = true，服务仍可启动（降级为哈希嵌入）"
  fi
fi

echo "== 4. 配置一致性 =="
if [ -f "$CFG" ]; then
  ok "$CFG 存在"
  PRIMARY=$(cfg "llm" "primary")
  echo "         LLM primary = ${PRIMARY:-未设置}"
  if [ "$(cfg "llm.gguf" "use_gpu")" = "false" ]; then
    ok "use_gpu = false（纯 CPU）"
  else
    warn "use_gpu 未设为 false，可能尝试 GPU 卸载"
  fi
  TH=$(cfg "llm.gguf" "threads")
  CORES=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo "?")
  echo "         threads=${TH:-未设置}，逻辑核心=${CORES}（建议 = 物理核心数，通常为逻辑核心的一半）"
  if [ "$(cfg "llm.gguf" "n_gpu_layers")" = "0" ]; then
    ok "n_gpu_layers = 0"
  else
    warn "n_gpu_layers 非 0"
  fi
else
  fail "缺少配置文件：$CFG"
fi

echo "== 5. 端口 =="
if curl -sS --max-time 2 http://127.0.0.1:3000/health >/dev/null 2>&1; then
  warn "3000 端口已有服务在跑（可能重复启动，或上一实例未退出）"
else
  ok "3000 端口空闲"
fi

echo "== 6. 构建产物与 llama.cpp 指令集 =="
BIN=""
for p in target/release/shouxin-server.exe target/release/shouxin-server \
         target/debug/shouxin-server.exe target/debug/shouxin-server; do
  [ -f "$p" ] && BIN="$p" && break
done
if [ -n "$BIN" ]; then
  ok "$BIN（$(( $(stat -c %s "$BIN" 2>/dev/null || stat -f %z "$BIN")/1048576 )) MB）"
else
  fail "未找到二进制，先构建： bash scripts/build_gguf.sh"
fi

# 取最新的一个 llama.cpp 构建目录（避免多目录重复计数造成误判）
BDIR=""
for d in target/release/build/llama-cpp-sys-2-*/out/build target/debug/build/llama-cpp-sys-2-*/out/build; do
  if [ -f "$d/build.ninja" ]; then BDIR="$d"; fi
done
if [ -n "$BDIR" ] && [ -f "$BDIR/build.ninja" ]; then
  GENERIC=$(grep -c "GGML_CPU_GENERIC" "$BDIR/build.ninja" 2>/dev/null | head -1 | tr -dc '0-9')
  ARCH=$(grep -oE "/arch:[A-Za-z0-9]+" "$BDIR/build.ninja" 2>/dev/null | sort -u | tr '\n' ' ')
  if [ "${GENERIC:-1}" = "0" ] && [ -n "$ARCH" ]; then
    ok "llama.cpp 指令集: $ARCH（无 GGML_CPU_GENERIC）"
  else
    fail "llama.cpp 退化为标量实现（GGML_CPU_GENERIC=${GENERIC:-?}, ARCH=${ARCH:-无}）→ 推理会慢 10~20 倍，见 docs/PERFORMANCE.md"
  fi
else
  warn "未找到 llama.cpp 构建目录（未构建 gguf 特性？）"
fi

echo ""
echo "== 汇总：OK=$OK  WARN=$WARN  FAIL=$FAIL =="
[ "$FAIL" -eq 0 ] || exit 1
