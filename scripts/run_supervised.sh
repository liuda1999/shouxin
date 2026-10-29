#!/usr/bin/env bash
# 压测守护：服务崩溃（llama.cpp 分词异常会 abort 整个进程）时自动重启。
#
# 背景：MSVC 的 std::regex 在个别极端输入上会抛 regex_error，
# C++ 异常跨 FFI 抛回 Rust 无法捕获，进程直接 abort。
# 抽取任务持久化在 sled 里，重启后会自动恢复，所以「重启」是安全的，
# 而且队列侧对"连续崩溃的任务"做了次数上限，不会无限循环。
#
# 用法：
#   bash scripts/run_supervised.sh [配置] [数据目录] [端口]
# 停止：
#   touch .stop_stress   （当前进程退出后不再拉起）

set -u
cd "$(dirname "${BASH_SOURCE[0]}")/.."

CFG="${1:-config.stress.toml}"
DATA_DIR="${2:-./data_stress2}"
PORT="${3:-127.0.0.1:3000}"
LOG=".server_run.log"
SUP=".supervisor.log"

: > "$LOG"

rapid=0
while true; do
  if [ -f .stop_stress ]; then
    echo "[supervisor] $(date +%H:%M:%S) 收到停止标记，退出" >> "$SUP"
    break
  fi

  start=$(date +%s)
  SHOUXIN_CONFIG="$CFG" ./target/debug/shouxin-server.exe \
    --data-dir "$DATA_DIR" --host "$PORT" >> "$LOG" 2>&1
  code=$?
  dur=$(( $(date +%s) - start ))

  # 队列里的"连续崩溃任务"上限是 3 次，加上队列本身有进度，
  # 正常情况不会出现"秒退"。连续 3 次秒退就认为环境有问题，停下来人工看。
  if [ "$dur" -lt 15 ]; then
    rapid=$((rapid + 1))
  else
    rapid=0
  fi
  echo "[supervisor] $(date +%H:%M:%S) 进程退出 code=$code 运行 ${dur}s（连续秒退 $rapid 次）" >> "$SUP"

  if [ "$rapid" -ge 3 ]; then
    echo "[supervisor] 连续秒退，停止自动重启，请检查 $LOG" >> "$SUP"
    break
  fi

  sleep 5
done
