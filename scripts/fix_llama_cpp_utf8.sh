#!/usr/bin/env bash
# 修正 llama.cpp 源码中的非 ASCII 字符（MSVC 按 GBK 代码页解析会报 C2001）
#
# 背景：llama-cpp-sys 依赖的 llama.cpp 里，common/jinja/utils.h 含有一个
# UTF-8 箭头字符（U+21B5 "↵"）。MSVC 在中文 Windows 上默认使用代码页 936，
# 会把这个多字节序列误判，导致 "error C2001: newline in constant"。
# 由于本机沙箱禁止调用 reg.exe，无法使用依赖注册表的 VS 生成器，
# 因此这里直接把该字符替换为 ASCII "->"（仅影响调试输出的箭头符号）。
#
# 幂等：已替换过则不会重复处理。
set -euo pipefail

CRATE_DIR="$(ls -d "$HOME"/.cargo/registry/src/*/llama-cpp-sys-2-0.1.146 2>/dev/null | head -1 || true)"

if [ -z "$CRATE_DIR" ]; then
  echo "未找到 llama-cpp-sys-2-0.1.146 源码目录，跳过（可能尚未下载）"
  exit 0
fi

HEADER="$CRATE_DIR/llama.cpp/common/jinja/utils.h"

if [ ! -f "$HEADER" ]; then
  echo "未找到 $HEADER，跳过"
  exit 0
fi

if grep -q 'string_replace_all(substr, "\\n", "->")' "$HEADER"; then
  echo "已处理过：$HEADER"
  exit 0
fi

python - "$HEADER" <<'PY'
import sys
path = sys.argv[1]
with open(path, 'rb') as f:
    data = f.read()
# U+21B5 (↵) -> "->"
data = data.replace(b'\xe2\x86\xb5', b'->')
with open(path, 'wb') as f:
    f.write(data)
print("patched:", path)
PY
