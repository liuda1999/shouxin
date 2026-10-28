# 构建指南（Build）

守心是纯 Rust 项目，但 `gguf` 特性会把 `llama.cpp`（C/C++）一起编进来，因此需要 C++ 工具链。

| 构建目标 | 命令 | 依赖 | 资源占用 |
|----------|------|------|----------|
| 默认（仅 HTTP + 嵌入，无本地 LLM） | `cargo build --release` | Rust | 不加载 LLM |
| **本地 GGUF 推理（推荐）** | `cargo build --release --features gguf` | Rust + MSVC/Clang + CMake + Ninja | **纯 CPU，不占显存** |
| GPU 卸载（可选） | `cargo build --release --features gguf-cuda` | 上面 + CUDA Toolkit | 占显存；CPU 线程 ≤ 60% |

> 项目默认**纯 CPU 推理**：`gguf` 特性刻意**不编译 CUDA 后端**，
> 二进制更小、构建快约 10 倍、运行时不依赖 `cublas64_12.dll`。
>
> 这是有意的资源策略 —— 守心是后台服务，**显存是稀缺资源，默认一律不占用**：
> `use_gpu` 在配置缺省与代码缺省下都是 `false`，`n_gpu_layers` 恒为 0。
> 确实需要 GPU 时才用 `gguf-cuda` 编译，并显式 `use_gpu = true`；
> 此时线程数会被硬性限制在 `核心数 × gpu_max_cpu_percent%`（默认 60%）以内。

---

## 1. 前置依赖

### 1.1 全平台

- **Rust** ≥ 1.75（`rustup` 安装即可）
- **CMake** ≥ 3.20
- **Ninja**（推荐；用 Makefile 也能编，但慢）

### 1.2 Windows

- **Visual Studio 2022 Build Tools**（勾选 "使用 C++ 的桌面开发"），或完整 VS 2022
- 确认 `cl.exe`、`cmake.exe`、`ninja.exe` 可用

### 1.3 Linux / macOS

```bash
# Debian/Ubuntu
sudo apt install build-essential cmake ninja-build pkg-config libssl-dev

# macOS
brew install cmake ninja
```

---

## 2. Windows 一键构建（推荐）

仓库根目录：

```bash
bash scripts/build_gguf.sh                    # debug
bash scripts/build_gguf.sh --release          # release
SHOUXIN_BIN=bench_gguf bash scripts/build_gguf.sh          # 编译基准工具
SHOUXIN_FEATURES=gguf-cuda bash scripts/build_gguf.sh      # GPU 版（需 CUDA Toolkit）
```

### 2.0 工具链从哪来（自动探测，仓库不写死路径）

`build_gguf.sh` 只做编排，具体路径由 `scripts/dev_env.sh` 探测：

| 组件 | 探测顺序 |
|------|----------|
| MSVC | `vswhere.exe` → 扫描 `C://Program Files\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*` 取最新 |
| Windows SDK | `C://Program Files (x86)\Windows Kits\10\Include\` 下最新版本 |
| cmake | `PATH` → `C://Program Files\CMake` → VS 自带 → pip 装的 cmake |
| ninja | `PATH` → VS 自带 → cmake 同目录 / Python Scripts |

工具装在非标准位置时（例如便携版 Python 环境），复制模板填写即可（该文件不进 Git）：

```bash
cp scripts/toolchain.local.sh.example scripts/toolchain.local.sh
# SHOUXIN_CMAKE="/d/portable-python/Lib/site-packages/cmake/data/bin/cmake.exe"
# SHOUXIN_NINJA="/d/portable-python/Scripts/ninja.exe"
```

也可临时用环境变量覆盖：`SHOUXIN_MSVC_ROOT`、`SHOUXIN_SDK_VERSION`、`SHOUXIN_CMAKE`、`SHOUXIN_NINJA`、`SHOUXIN_CUDA_ROOT`。
探测失败时脚本会直接告诉你缺什么、怎么装，而不是静默退出。

脚本做的事（每一步都是踩过的坑，详见注释）：

1. 手工拼装 MSVC / Windows SDK 的 `INCLUDE` / `LIB` / `PATH`（不依赖 `vcvars64.bat`）；
2. 用 **Ninja 生成器**，并按 **Windows 绝对路径**指定 `cmake` / `ninja` / `cl.exe`
   —— `cargo` 构建脚本内部的 `PATH` 是 POSIX 形态（`/d/...`），Windows 版 cmake 解析不了，
   表现为**静默退出 1**，极难排查；
3. 注入 `scripts/llama_cpu_toolchain.cmake`，强制 `GGML_AVX2=ON` **并把 `/arch:AVX2` 写进
   编译命令**（只设 CMake 变量不会生效，实测 ggml 会退化成 SSE2 标量实现，慢 10~20 倍）；
4. `-utf-8` 传 MSVC（源码含中文/Unicode 字符时，GBK 代码页会报 `C2001: newline in constant`）；
5. `MSYS2_ARG_CONV_EXCL="*"` 防止 Git Bash 把 `/utf-8` 之类的参数当路径转换；
6. 最后执行 `cargo build --features gguf --bin shouxin-server`。

构建完成后产物在 `target/debug/shouxin-server.exe`（或 `--release` 下的 `target/release/`）。

### 2.1 想要 Release 二进制

```bash
bash scripts/build_gguf.sh --release
```

### 2.2 想跑任意 cargo 命令（含测试）

`scripts/build_gguf.sh` 只编 `shouxin-server`。要跑其它 cargo 子命令（如带 `gguf` 特性跑测试），
先把环境导入当前 shell：

```bash
source scripts/dev_env.sh
cargo test --features gguf --lib
cargo build --release --features gguf --bin bench_gguf
```

### 2.3 验证 AVX2 真的生效

这是判断"CPU 推理为什么慢"的第一现场：

```bash
B=$(ls -d target/debug/build/llama-cpp-sys-2-*/out/build | head -1)
grep -oE "/arch:[A-Za-z0-9]+" "$B/build.ninja" | sort -u
# 期望输出：/arch:AVX2
```

若为空 → ggml 走的是 SSE2 标量路径，推理会慢一个数量级。

---

## 3. Linux / macOS 构建

```bash
# 纯 CPU（推荐）
cargo build --release --features gguf

# 用本机全部指令集（GCC/Clang 支持 -march=native，比 MSVC 省事）
RUSTFLAGS="-C target-cpu=native" cargo build --release --features gguf
```

GCC/Clang 下 `llama-cpp-sys` 的 `GGML_NATIVE` 会正常生效，**不需要**工具链补丁文件。

---

## 4. 运行

```bash
# 默认监听 127.0.0.1:3000，读取 ./config.toml
./target/release/shouxin-server

# 指定监听地址
./target/release/shouxin-server --host 0.0.0.0:3000

# 指定配置文件（也可用环境变量 SHOUXIN_CONFIG）
SHOUXIN_CONFIG=./config.prod.toml ./target/release/shouxin-server
```

启动成功的日志特征：

```
[GGUF] 初始化: GPU=false, n_gpu_layers=0, context=4096, threads=16
GGUF model loaded successfully
Embedder initialized (backend: fastembed, model: intfloat/multilingual-e5-base)
Server listening on 127.0.0.1:3000
```

健康检查：

```bash
curl http://127.0.0.1:3000/health
# {"status":"ok","time":"..."}
```

---

## 5. 测试

```bash
# 单元测试（快，不加载模型）
cargo test --lib

# 集成测试（会加载真实 ONNX 嵌入模型，注意耗时）
cargo test --tests
```

集成测试需要 `.fastembed_cache/` 里已有嵌入模型；没有时会走哈希降级（结果仍可通过，
但语义检索类断言会退化）。

**全量 API 冒烟**（覆盖 `routes.rs` 注册的全部端点，需先启动服务）：

```bash
bash scripts/api_smoke_test.sh
# 指定地址与隔离用户
BASE=http://127.0.0.1:3001 USER_ID=smoke bash scripts/api_smoke_test.sh
```

当前基线：`PASS=35  FAIL=0  SKIP=0`（Gemma 与 Ministral 两套配置均通过）。

> ⚠️ **踩过的坑：`cargo test` 会用「默认特性」重编二进制，把 `target/debug/shouxin-server`
> 覆盖成不带 `gguf` 的版本**，之后再启动就会报：
> `GGUF 后端未编译。请使用 cargo build --features gguf 启用`。
> 跑完测试要重新执行一次 `bash scripts/build_gguf.sh`。
> 想避免覆盖可以用 `cargo test --features gguf`（编译更久）。

---

## 6. 常见构建错误对照

| 报错 | 原因 | 处理 |
|------|------|------|
| `error C2001: newline in constant` | MSVC 按 GBK 解析 UTF-8 源码 | 传 `-utf-8`（脚本已做），或跑 `scripts/fix_llama_cpp_utf8.sh` |
| `is \`cmake\` not installed?` / 构建脚本静默退出 1 | cmake 不在 PATH，或 PATH 是 POSIX 形态 | 用 `scripts/build_gguf.sh`（内部用 Windows 绝对路径） |
| `Failed to run \`reg.exe\`` / cmake 卡在探测 VS | VS 生成器要读注册表 | 用 Ninja 生成器 + 显式 `cl.exe`（脚本已做） |
| `schannel: ... CRYPT_E_NO_REVOCATION_CHECK` | TLS 吊销检查被环境阻断 | `git -c http.schannelCheckRevoke=false clone ...`，curl 加 `--ssl-no-revoke` |
| `error LNK1104: 无法打开文件 cublas64_12.lib` | 用了 `gguf-cuda` 但没有 CUDA | 改用 `--features gguf`（纯 CPU） |
| 编译很久 | 首次要编整个 llama.cpp | 之后增量编译只需 1 分钟左右 |

---

## 7. 构建产物说明

| 文件 | 说明 |
|------|------|
| `target/{debug,release}/shouxin-server` | 服务二进制（`gguf` 特性下内含 llama.cpp 静态库） |
| `target/*/build/llama-cpp-sys-2-*/out/build/build.ninja` | llama.cpp 的实际编译参数（查 AVX2 就看这里） |
| `.fastembed_cache/` | 嵌入模型缓存（不进 Git） |
| `models/` | GGUF 模型（不进 Git） |
