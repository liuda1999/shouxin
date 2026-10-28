#!/usr/bin/env bash
# 开发环境准备（Windows + MSVC）
#
# 作用：把编译 llama.cpp 所需的 MSVC / Windows SDK / CMake / Ninja 环境导出到当前 shell，
# 这样任意 cargo 命令都能跑，而不仅限于 scripts/build_gguf.sh。
#
# 用法：
#   source scripts/dev_env.sh
#   cargo test --features gguf --lib
#   cargo build --release --features gguf
#
# 设计原则：**不写死任何机器相关路径**。全部靠自动探测，探测不到时给出可操作的提示，
# 并允许用环境变量覆盖：
#   SHOUXIN_MSVC_ROOT    MSVC 工具集目录（...\VC\Tools\MSVC\14.xx.xxxxx）
#   SHOUXIN_SDK_VERSION  Windows SDK 版本（如 10.0.26100.0）
#   SHOUXIN_CMAKE        cmake 可执行文件
#   SHOUXIN_NINJA        ninja 可执行文件
#   SHOUXIN_CUDA_ROOT    CUDA 根目录（仅 gguf-cuda 需要）
#
# 背景（Windows 上的两个坑）：
#   1. cmake 的 "Visual Studio" 生成器需要读注册表，某些受限环境禁止 reg.exe → 改用 Ninja 生成器；
#   2. 没有 vcvars 环境时，需手工拼 INCLUDE / LIB / PATH，并给 cmake 显式指定 cl.exe，
#      否则它仍会尝试探测 MSVC（读注册表）。

# 只在 Windows(Git Bash/MSYS) 下生效；其它平台直接返回
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) ;;
  *) echo "[dev_env] 非 Windows 环境，跳过（Linux/macOS 直接 cargo build 即可）"; return 0 2>/dev/null || exit 0 ;;
esac

_dev_env_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# 本地覆盖（不纳入 Git）：用于记录本机特有的工具链路径。
# 复制 scripts/toolchain.local.sh.example 为 scripts/toolchain.local.sh 后按需修改。
if [ -f "$_dev_env_root/scripts/toolchain.local.sh" ]; then
  # shellcheck disable=SC1091
  . "$_dev_env_root/scripts/toolchain.local.sh"
fi

to_posix() { # C:\a\b -> /c/a/b
  local p="$1"
  if command -v cygpath >/dev/null 2>&1; then cygpath -u "$p"; else
    printf '%s' "$p" | sed -e 's|\\|/|g' -e 's|^\([A-Za-z]\):|/\L\1|'
  fi
}
to_win() { # /c/a/b -> C:\a\b
  local p="$1"
  if command -v cygpath >/dev/null 2>&1; then cygpath -w "$p"; else
    printf '%s' "$p" | sed -e 's|^/\([A-Za-z]\)/|\1:/|' -e 's|/|\\|g'
  fi
}

# ---------- 1. MSVC 工具集 ----------
VS_INSTALL=""
if [ -n "${SHOUXIN_MSVC_ROOT:-}" ]; then
  MSVC_ROOT="$SHOUXIN_MSVC_ROOT"
else
  VSWHERE="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
  [ -f "$VSWHERE" ] || VSWHERE="/c/Program Files/Microsoft Visual Studio/Installer/vswhere.exe"
  if [ -f "$VSWHERE" ]; then
    VS_INSTALL="$("$VSWHERE" -latest -products '*' \
      -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 \
      -property installationPath 2>/dev/null | tr -d '\r' | head -1)"
  fi
  if [ -z "$VS_INSTALL" ]; then
    # 兜底：直接扫安装目录（取版本号最大的一个）
    VS_INSTALL="$(ls -d "/c/Program Files/Microsoft Visual Studio"/*/*/ 2>/dev/null \
      | sed 's|/$||' | sort -V | tail -1)"
  else
    VS_INSTALL="$(to_posix "$VS_INSTALL")"
  fi

  if [ -n "$VS_INSTALL" ] && [ -d "$VS_INSTALL/VC/Tools/MSVC" ]; then
    MSVC_VER_DIR="$(ls -d "$VS_INSTALL/VC/Tools/MSVC"/*/ 2>/dev/null | sed 's|/$||' | sort -V | tail -1)"
    [ -n "$MSVC_VER_DIR" ] && MSVC_ROOT="$(to_win "$MSVC_VER_DIR")"
  fi
fi

if [ -z "${MSVC_ROOT:-}" ] || [ ! -d "$(to_posix "$MSVC_ROOT")" ]; then
  echo "[dev_env] 未找到 MSVC C++ 工具集。" >&2
  echo "          请安装 Visual Studio Build Tools（勾选 \"使用 C++ 的桌面开发\"），" >&2
  echo "          或设置 SHOUXIN_MSVC_ROOT 指向 ...\\VC\\Tools\\MSVC\\14.xx.xxxxx" >&2
  return 1 2>/dev/null || exit 1
fi

# ---------- 2. Windows SDK ----------
SDK_BASE="/c/Program Files (x86)/Windows Kits/10"
if [ -n "${SHOUXIN_SDK_VERSION:-}" ]; then
  SDK_VER="$SHOUXIN_SDK_VERSION"
else
  # 注意：路径含空格，不能用 xargs basename（会按空格切分），用 sed 取末段
  SDK_VER="$(ls -d "$SDK_BASE/Include"/*/ 2>/dev/null | sed 's|/$||' | sed 's|.*/||' | sort -V | tail -1)"
fi
if [ -z "${SDK_VER:-}" ] || [ ! -d "$SDK_BASE/Include/$SDK_VER" ]; then
  echo "[dev_env] 未找到 Windows SDK（$SDK_BASE/Include）。" >&2
  echo "          请在 VS Installer 中安装 Windows 10/11 SDK，或设置 SHOUXIN_SDK_VERSION" >&2
  return 1 2>/dev/null || exit 1
fi
SDK_ROOT="$(to_win "$SDK_BASE")"

# ---------- 3. CMake ----------
CMAKE_EXE=""
if [ -n "${SHOUXIN_CMAKE:-}" ]; then
  CMAKE_EXE="$SHOUXIN_CMAKE"
else
  for c in "$(command -v cmake 2>/dev/null)" \
           "/c/Program Files/CMake/bin/cmake.exe" \
           "${VS_INSTALL:-/nonexistent}/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe"; do
    [ -n "$c" ] && [ -x "$c" ] && CMAKE_EXE="$c" && break
  done
  # pip 安装的 cmake（data/bin 下才是真正的可执行文件，Scripts 下的常见为失效 shim）
  if [ -z "${CMAKE_EXE:-}" ]; then
    CMAKE_EXE="$(ls -d /c/Users/*/AppData/Roaming/Python/*/site-packages/cmake/data/bin/cmake.exe \
                        /c/Users/*/AppData/Local/Programs/Python/*/Lib/site-packages/cmake/data/bin/cmake.exe \
                        2>/dev/null | head -1)"
  fi
  # 再试：通过 python 反查 cmake 包位置（可覆盖任意盘符/自定义 Python 环境）
  if [ -z "${CMAKE_EXE:-}" ]; then
    for PY in python3 python py; do
      command -v "$PY" >/dev/null 2>&1 || continue
      CMAKE_EXE="$("$PY" -c "import os,cmake;print(os.path.join(os.path.dirname(cmake.__file__),'data','bin','cmake.exe'))" 2>/dev/null)"
      [ -n "${CMAKE_EXE:-}" ] && [ -x "$CMAKE_EXE" ] && break || CMAKE_EXE=""
    done
  fi
fi
if [ -z "${CMAKE_EXE:-}" ] || ! "$CMAKE_EXE" --version >/dev/null 2>&1; then
  echo "[dev_env] 未找到可用的 cmake（>=3.20）。" >&2
  echo "          安装：winget install Kitware.CMake  或  pip install cmake" >&2
  echo "          也可设置 SHOUXIN_CMAKE 指定路径" >&2
  return 1 2>/dev/null || exit 1
fi

# ---------- 4. Ninja ----------
NINJA_EXE=""
if [ -n "${SHOUXIN_NINJA:-}" ]; then
  NINJA_EXE="$SHOUXIN_NINJA"
else
  for n in "$(command -v ninja 2>/dev/null)" \
           "${VS_INSTALL:-/nonexistent}/Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe" \
           "$(dirname "${CMAKE_EXE:-/nonexistent}")/ninja.exe"; do
    [ -n "$n" ] && [ -x "$n" ] && NINJA_EXE="$n" && break
  done
  # python 环境的 Scripts 目录里通常自带 ninja
  if [ -z "${NINJA_EXE:-}" ]; then
    for PY in python3 python py; do
      command -v "$PY" >/dev/null 2>&1 || continue
      NINJA_EXE="$("$PY" -c "import sysconfig,os;print(os.path.join(sysconfig.get_path('scripts'),'ninja.exe'))" 2>/dev/null)"
      [ -n "${NINJA_EXE:-}" ] && [ -x "$NINJA_EXE" ] && break || NINJA_EXE=""
    done
  fi
fi
if [ -z "${NINJA_EXE:-}" ]; then
  echo "[dev_env] 未找到 ninja。" >&2
  echo "          安装：winget install Ninja-build.Ninja  或 设置 SHOUXIN_NINJA" >&2
  return 1 2>/dev/null || exit 1
fi

# ---------- 5. 导出环境 ----------
MSVC_POSIX="$(to_posix "$MSVC_ROOT")"

export INCLUDE="${MSVC_ROOT}\\include;${SDK_ROOT}\\Include\\${SDK_VER}\\ucrt;${SDK_ROOT}\\Include\\${SDK_VER}\\shared;${SDK_ROOT}\\Include\\${SDK_VER}\\um;${SDK_ROOT}\\Include\\${SDK_VER}\\winrt"
export LIB="${MSVC_ROOT}\\lib\\x64;${SDK_ROOT}\\Lib\\${SDK_VER}\\ucrt\\x64;${SDK_ROOT}\\Lib\\${SDK_VER}\\um\\x64"
export LIBPATH="$LIB"

export PATH="$MSVC_POSIX/bin/Hostx64/x64:$SDK_BASE/bin/${SDK_VER}/x64:$(dirname "$NINJA_EXE"):$(dirname "$CMAKE_EXE"):$HOME/.cargo/bin:$PATH"

export CMAKE_GENERATOR=Ninja
# cmake / ninja 用 Windows 绝对路径：cargo 构建脚本内部再拉起它们时，
# 依赖 POSIX 风格 PATH 会找不到可执行文件（表现为 cmake 静默退出 1）
export CMAKE="$(to_win "$(to_posix "$CMAKE_EXE")")"
export CMAKE_MAKE_PROGRAM="$(to_win "$(to_posix "$NINJA_EXE")")"

# 显式指定编译器，避免 cmake 用 Ninja 生成器时去探测 MSVC（读注册表）
CL_EXE="$MSVC_POSIX/bin/Hostx64/x64/cl.exe"
export CMAKE_C_COMPILER="$(to_win "$CL_EXE")"
export CMAKE_CXX_COMPILER="$CMAKE_C_COMPILER"
export CC="$CMAKE_C_COMPILER"
export CXX="$CMAKE_C_COMPILER"

# 必须留空 MSYS 的参数转换，否则 /utf-8 会被当成路径改写
export MSYS2_ARG_CONV_EXCL="*"

# 关键：target-cpu=native 会触发 llama-cpp-sys 的 GGML_NATIVE=ON；
# 否则 build.rs 写死 GGML_NATIVE=OFF，AVX2 全关，CPU 推理慢 20 倍
export RUSTFLAGS="${RUSTFLAGS:--C target-cpu=native}"

export CFLAGS="-utf-8"
export CXXFLAGS="-utf-8"
export CFLAGS_x86_64_pc_windows_msvc="-utf-8"
export CXXFLAGS_x86_64_pc_windows_msvc="-utf-8"

# 工具链文件按仓库相对位置解析，不写死项目绝对路径
export CMAKE_TOOLCHAIN_FILE="$_dev_env_root/scripts/llama_cpu_toolchain.cmake"

# ---------- 6. CUDA（可选，仅 gguf-cuda 需要） ----------
if [ -n "${SHOUXIN_CUDA_ROOT:-}" ]; then
  export CUDA_PATH="$SHOUXIN_CUDA_ROOT"
elif [ -z "${CUDA_PATH:-}" ] && [ -d "/c/Program Files/NVIDIA GPU Computing Toolkit/CUDA" ]; then
  CUDA_LATEST="$(ls -d "/c/Program Files/NVIDIA GPU Computing Toolkit/CUDA"/v*/ 2>/dev/null | sed 's|/$||' | sort -V | tail -1)"
  [ -n "$CUDA_LATEST" ] && export CUDA_PATH="$(to_win "$CUDA_LATEST")"
fi

echo "[dev_env] MSVC=$(basename "$MSVC_ROOT")  SDK=$SDK_VER"
echo "[dev_env] cmake=$CMAKE_EXE"
echo "[dev_env] ninja=$NINJA_EXE"
[ -n "${CUDA_PATH:-}" ] && echo "[dev_env] CUDA=$CUDA_PATH（仅 gguf-cuda 使用）"
echo "[dev_env] 环境已就绪"
