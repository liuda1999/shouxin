#!/usr/bin/env python3
"""分段并行 + 断点续传下载大模型文件（应对单连接被限速/被掐断的网络环境）

用法:
    python scripts/download_gguf.py <url> <输出文件> [并发数]

为什么需要它：
    * 国内直连 huggingface.co / hf-mirror 时，单连接常被限速到几十 KB/s；
    * 更糟的是长连接会被中途掐断，普通 curl 会静默产出**截断文件**
      （GGUF 头部正常，但加载时才报错）。本脚本把文件切成 N 段并行拉，
      每段按"已下载字节数"续传，直到字节数对得上为止。

安全性：
    结束前校验最终文件长度 == 服务端 Content-Length，不等则以非 0 退出。
"""
import os
import sys
import threading
import time

import requests
import urllib3

CHUNK = 1 << 20  # 1 MiB
MAX_STALLS = 12  # 单个分段连续无进展的重试次数上限


def head_size(url: str, session: requests.Session) -> int:
    r = session.head(url, allow_redirects=True, timeout=30)
    r.raise_for_status()
    n = r.headers.get("Content-Length")
    if not n:
        # 部分镜像不给 HEAD 长度，退回 GET 首字节探测
        r = session.get(url, headers={"Range": "bytes=0-0"}, stream=True, timeout=30)
        cr = r.headers.get("Content-Range", "")
        r.close()
        if "/" in cr:
            return int(cr.rsplit("/", 1)[1])
        raise RuntimeError("无法获取文件大小（服务端未返回 Content-Length）")
    return int(n)


def download_part(url, path, start, end, idx, state, lock, session):
    """下载 [start, end] 区间，支持多次续传直到长度正确"""
    part_file = f"{path}.part{idx}"
    expected = end - start + 1
    stalls = 0

    while True:
        have = os.path.getsize(part_file) if os.path.exists(part_file) else 0
        if have > expected:  # 异常脏数据，重下
            open(part_file, "wb").close()
            have = 0
        if have == expected:
            return

        headers = {"Range": f"bytes={start + have}-{end}"}
        before = have
        try:
            with session.get(url, headers=headers, stream=True, timeout=(30, 90)) as r:
                if r.status_code not in (200, 206):
                    raise RuntimeError(f"HTTP {r.status_code}")
                if r.status_code == 200 and have > 0:
                    # 服务端忽略了 Range，只能从头写，避免拼接错位
                    open(part_file, "wb").close()
                    have = 0
                with open(part_file, "ab") as f:
                    for chunk in r.iter_content(CHUNK):
                        if not chunk:
                            continue
                        f.write(chunk)
                        with lock:
                            state["bytes"] += len(chunk)
        except Exception as exc:  # noqa: BLE001
            print(f"[part {idx}] 中断，续传中: {type(exc).__name__}", flush=True)

        after = os.path.getsize(part_file) if os.path.exists(part_file) else 0
        if after <= before:
            stalls += 1
            if stalls > MAX_STALLS:
                print(f"[part {idx}] 连续 {MAX_STALLS} 次无进展，放弃", flush=True)
                with lock:
                    state["failed"] = True
                return
            time.sleep(2 + stalls)
        else:
            stalls = 0


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(2)

    url = sys.argv[1]
    out = sys.argv[2]
    parts = int(sys.argv[3]) if len(sys.argv) > 3 else 24

    urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)
    session = requests.Session()
    session.headers["User-Agent"] = "shouxin-downloader/1.1"
    adapter = requests.adapters.HTTPAdapter(
        pool_connections=parts + 8, pool_maxsize=parts + 8, max_retries=0
    )
    session.mount("https://", adapter)
    session.mount("http://", adapter)
    # 本地代理常做 TLS 中间人，证书校验会失败；文件完整性由长度校验兜底
    session.verify = os.environ.get("GGUF_TLS_VERIFY", "0") != "0"

    size = head_size(url, session)
    print(f"目标: {out}", flush=True)
    print(f"大小: {size / 1024 ** 2:.0f} MB，分 {parts} 段下载", flush=True)

    # 预分配（不覆盖已有正确长度的内容）
    if not os.path.exists(out) or os.path.getsize(out) != size:
        with open(out, "wb") as f:
            f.truncate(size)

    seg = size // parts
    state = {"bytes": 0, "failed": False}
    lock = threading.Lock()

    threads = []
    for i in range(parts):
        start = i * seg
        end = size - 1 if i == parts - 1 else (i + 1) * seg - 1
        t = threading.Thread(
            target=download_part,
            args=(url, out, start, end, i, state, lock, session),
            daemon=True,
        )
        t.start()
        threads.append(t)

    t0 = time.time()
    while any(t.is_alive() for t in threads):
        time.sleep(15)
        el = time.time() - t0
        speed = state["bytes"] / el / 1024 if el > 0 else 0
        pct = state["bytes"] / size * 100
        print(
            f"进度 {pct:5.1f}%  {state['bytes'] / 1024 ** 2:7.0f}/{size / 1024 ** 2:.0f} MB"
            f"  {speed:6.0f} KB/s",
            flush=True,
        )

    if state["failed"]:
        print("有分段失败，保留 .partN 文件，重跑本脚本可继续", flush=True)
        sys.exit(1)

    print("合并分段...", flush=True)
    with open(out, "wb") as final:
        for i in range(parts):
            pf = f"{out}.part{i}"
            with open(pf, "rb") as src:
                while True:
                    buf = src.read(CHUNK * 8)
                    if not buf:
                        break
                    final.write(buf)
            os.remove(pf)

    final_size = os.path.getsize(out)
    if final_size != size:
        print(f"校验失败: {final_size} != {size}", flush=True)
        sys.exit(1)
    print(f"完成并校验通过: {out} ({final_size} 字节)", flush=True)


if __name__ == "__main__":
    main()
