"""实测进程 CPU 占用：验证"本项目 ≤ 60% 整机"是否真的生效。

用 GetProcessTimes 采样进程内核+用户态 CPU 时间，按 采样间隔 × 逻辑核数 折算占用率。
同时在采样期间持续投喂长对话制造推理负载，避免"空转 0% 也算通过"。
"""
import ctypes
import os
import ctypes.wintypes as wt
import json
import subprocess
import threading
import time
import urllib.request

KERNEL32 = ctypes.windll.kernel32
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
LOGICAL_CORES = os.cpu_count() or 16


class FILETIME(ctypes.Structure):
    _fields_ = [("dwLowDateTime", wt.DWORD), ("dwHighDateTime", wt.DWORD)]


def filetime_to_int(ft):
    return (ft.dwHighDateTime << 32) | ft.dwLowDateTime


def cpu_times(pid):
    h = KERNEL32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not h:
        return None
    try:
        creation, exit_, kernel, user = FILETIME(), FILETIME(), FILETIME(), FILETIME()
        ok = KERNEL32.GetProcessTimes(h, ctypes.byref(creation), ctypes.byref(exit_),
                                      ctypes.byref(kernel), ctypes.byref(user))
        if not ok:
            return None
        return (filetime_to_int(kernel) + filetime_to_int(user)) / 1e7  # 秒
    finally:
        KERNEL32.CloseHandle(h)


def find_pid():
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq shouxin-server.exe", "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) > 1 and parts[0].lower().startswith("shouxin"):
            return int(parts[1])
    return None


def feed_load():
    texts = [
        "上午和张总过了一遍渠道复盘的结论，华东区回款慢了，要先解决经销商压货的问题，下周三之前给方案。",
        "下午面试了两个人，一个做过三年推荐系统，另一个偏工程，我倾向第二个，但要等用人部门确认编制。",
        "晚上把季度预算又改了一版，市场费用砍了百分之十五，研发增加了两个人力，明天上午发财务复核。",
        "客户那边反馈接口超时，排查下来是连接池配置太小，临时扩到两百，观察一天再决定是否改架构。",
        "和老同学吃饭，他说他们公司在做海外仓，问我们有没有合作可能，让我先发一份资料过去看看。",
        "这周要把供应商合同重新签一遍，法务要求补充数据合规条款，大概有七家在名单里。",
    ]
    for t in texts:
        try:
            req = urllib.request.Request("http://127.0.0.1:3000/api/memory/add",
                                         data=json.dumps({"conversation": t, "user_id": "cpucap"}).encode(),
                                         headers={"Content-Type": "application/json"}, method="POST")
            urllib.request.urlopen(req, timeout=30).read()
        except Exception as e:
            print("feed error:", e)
        time.sleep(1.5)


def main():
    pid = find_pid()
    if not pid:
        print("找不到进程")
        return 1

    t = threading.Thread(target=feed_load, daemon=True)
    t.start()

    prev = cpu_times(pid)
    prev_t = time.time()
    samples = []
    print(f"PID={pid} 逻辑核={LOGICAL_CORES}，开始采样 150s…")
    for i in range(30):
        time.sleep(5)
        cur, now = cpu_times(pid), time.time()
        if cur is None or prev is None:
            break
        pct = (cur - prev) / ((now - prev_t) * LOGICAL_CORES) * 100
        samples.append(pct)
        print(f"  [{i*5+5:>3}s] {pct:5.1f}%")
        prev, prev_t = cur, now

    if samples:
        peak = max(samples)
        avg = sum(samples) / len(samples)
        over = [s for s in samples if s > 62]
        print(f"\n峰值 {peak:.1f}% / 均值 {avg:.1f}% / 超 62% 的采样 {len(over)}/{len(samples)}")
        print("RESULT:", "PASS" if peak <= 62 and avg <= 60 else "FAIL")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
