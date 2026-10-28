#!/usr/bin/env bash
# 环境自检：部署前跑一遍，缺什么直接给出修复命令
#
# 用法： bash scripts/check_env.sh
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

OK=0
WARN=0
FAIL=0

ok()   { echo "  [OK]   $*"; OK=$((OK+1)); }
warn() { echo "  [WARN] $*"; WARN=$((WARN+1)); }
fail() { echo "  [FAIL] $*"; FAIL=$((FAIL+1)); }

echo "== 1. 工具链 =="
if command -v cargo >/dev/null 2>&1; then
  ok "cargo $(cargo --version 2>&1 | awk '{print $2}')"
else
  fail "未找到 cargo（安装：https://rustup.rs）"
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
MODEL=$(grep -E '^model_path' config.toml 2>/dev/null | head -1 | sed 's/.*= *"//;s/"$//')
MODEL="${MODEL:-./models/model.gguf}"
if [ -f "$MODEL" ]; then
  SIZE=$(stat -c %s "$MODEL" 2>/dev/null || stat -f %z "$MODEL" 2>/dev/null)
  if [ "$(head -c 4 "$MODEL" 2>/dev/null)" = "GGUF" ] || [ "$(xxd -l 4 -p "$MODEL" 2>/dev/null)" = "47475546" ]; then
    ok "$MODEL（$((SIZE/1048576)) MB，GGUF 头正确）"
  else
    fail "$MODEL 头部不是 GGUF —— 文件可能损坏/被截断，请重新下载"
  fi
else
  fail "缺少推理模型：$MODEL"
  echo "         下载见 docs/MODEL_SETUP.md，或："
  echo "         python scripts/download_gguf.py \"<hf-url>\" \"$MODEL\" 32"
fi

echo "== 3. 嵌入模型（ONNX）=="
CACHE=$(grep -E '^model_path' config.toml 2>/dev/null | sed -n '2p' | sed 's/.*= *"//;s/"$//')
CACHE="${CACHE:-.fastembed_cache}"
EMODEL=$(grep -E '^model = ' config.toml 2>/dev/null | sed -n '2p' | sed 's/.*= *"//;s/"$//')
EMODEL="${EMODEL:-intfloat/multilingual-e5-base}"
ONNX="$CACHE/$EMODEL/onnx/model.onnx"
if [ -f "$ONNX" ]; then
  ok "$ONNX（$(( $(stat -c %s "$ONNX" 2>/dev/null || stat -f %z "$ONNX")/1048576 )) MB）"
else
  warn "缺少嵌入模型：$ONNX"
  echo "         下载： bash scripts/fetch_embedding_model.sh"
  echo "         若 [embedder].fallback_to_hash = true，服务仍可启动（降级为哈希嵌入）"
fi

echo "== 4. 配置一致性 =="
if [ -f config.toml ]; then
  ok "config.toml 存在"
  grep -qE '^use_gpu *= *false' config.toml && ok "use_gpu = false（纯 CPU）" || warn "use_gpu 未设为 false，可能尝试 GPU 卸载"
  TH=$(grep -E '^threads' config.toml | head -1 | sed 's/[^0-9]//g')
  if [ -n "$TH" ]; then
    CORES=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo "?")
    echo "         threads=$TH，逻辑核心=$CORES（建议 = 物理核心数）"
  fi
  grep -qE '^n_gpu_layers *= *0' config.toml && ok "n_gpu_layers = 0" || warn "n_gpu_layers 非 0"
else
  fail "缺少 config.toml"
fi

echo "== 5. 端口 =="
if curl -sS --max-time 2 http://127.0.0.1:3000/health >/dev/null 2>&1; then
  warn "3000 端口已有服务在跑（可能重复启动）"
else
  ok "3000 端口空闲"
fi

echo "== 6. 构建产物 =="
BIN=""
for p in target/release/shouxin-server target/debug/shouxin-server target/release/shouxin-server.exe target/debug/shouxin-server.exe; do
  [ -f "$p" ] && BIN="$p" && break
done
if [ -n "$BIN" ]; then
  ok "$BIN（$(( $(stat -c %s "$BIN" 2>/dev/null || stat -f %z "$BIN")/1048576 )) MB）"
  B=$(ls -d target/*/build/llama-cpp-sys-2-*/out/build 2>/dev/null | head -1)
  if [ -n "$B" ] && [ -f "$B/build.ninja" ]; then
    G=$(grep -c "GGML_CPU_GENERIC" "$B/build.ninja" 2>/dev/null || echo 0)
    A=$(grep -oE "/arch:[A-Za-z0-9]+" "$B/build.ninja" 2>/dev/null | sort -u | tr '\n' ' ')
    if [ "$G" = "0" ] && [ -n "$A" ]; then
      ok "llama.cpp 指令集: $A（无 GGML_CPU_GENERIC）"
    else
      fail "llama.cpp 可能退化为标量实现（GGML_CPU_GENERIC=$G, ARCH=$A）—— 见 docs/PERFORMANCE.md"
    fi
  fi
else
  fail "未找到二进制，先构建： bash scripts/build_gguf.sh"
fi

echo ""
echo "== 汇总：OK=$OK  WARN=$WARN  FAIL=$FAIL =="
[ "$FAIL" -eq 0 ] || exit 1
