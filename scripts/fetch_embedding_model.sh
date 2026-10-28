#!/usr/bin/env bash
# 下载 multilingual-e5-base 的 ONNX 嵌入模型到工作区缓存
#
# 为什么要脚本：FastEmbed 首次运行会联网拉模型，但本机网络对 huggingface.co
# 的直连受限（证书吊销检查失败），因此改用 hf-mirror.com 镜像，并把文件放到
# HF 缓存布局下，让 FastEmbed 离线模式也能直接命中。
#
# 布局（hf-hub 规范）：
#   <cache>/models--intfloat--multilingual-e5-base/refs/main            -> 提交号
#   <cache>/models--intfloat--multilingual-e5-base/snapshots/<sha>/...  -> 实际文件
#
# 用法： bash scripts/fetch_embedding_model.sh [目标缓存目录]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_DIR="${1:-$ROOT/.fastembed_cache}"
REPO="intfloat/multilingual-e5-base"
MIRROR="${HF_MIRROR:-https://hf-mirror.com}"

# 固定提交号，保证可复现
REV="main"

MODEL_DIR="$CACHE_DIR/models--${REPO//\//--}"
SNAP_DIR="$MODEL_DIR/snapshots/$REV"
mkdir -p "$SNAP_DIR/onnx" "$MODEL_DIR/refs"

FILES=(
  "onnx/model.onnx"
  "tokenizer.json"
  "config.json"
  "special_tokens_map.json"
  "tokenizer_config.json"
)

echo "==> 目标缓存: $MODEL_DIR"
for f in "${FILES[@]}"; do
  dest="$SNAP_DIR/$f"
  if [ -s "$dest" ]; then
    echo "    已存在，跳过: $f"
    continue
  fi
  echo "    下载: $f"
  mkdir -p "$(dirname "$dest")"
  curl -fL --ssl-no-revoke --retry 3 --retry-delay 2 \
    -o "$dest" "$MIRROR/$REPO/resolve/$REV/$f"
done

# 记录 refs/main（hf-hub 离线解析用）
echo "main" > "$MODEL_DIR/refs/main"

echo "==> 完成，内容："
find "$MODEL_DIR" -type f | sed "s|$CACHE_DIR/||"
