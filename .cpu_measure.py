# -*- coding: utf-8 -*-
"""测量推理期间：本项目占整机 CPU 比例 + 整机总占用"""
import ctypes
import ctypes.wintypes as wt
import json
import os
import subprocess
import threading
import time
import urllib.request

K = ctypes.windll.kernel32


class FT(ctypes.Structure):
    _fields_ = [("lo", wt.DWORD), ("hi", wt.DWORD)]


def ft(x):
    return (x.hi << 32) | x.lo


def proc_cpu(pid):
    h = K.OpenProcess(0x1000, False, pid)
    if not h:
        return None
    a, b, k, u = FT(), FT(), FT(), FT()
    ok = K.GetProcessTimes(h, ctypes.byref(a), ctypes.byref(b),
                           ctypes.byref(k), ctypes.byref(u))
    K.CloseHandle(h)
    return (ft(k) + ft(u)) / 1e7 if ok else None


def sys_times():
    idle, kernel, user = FT(), FT(), FT()
    K.GetSystemTimes(ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user))
    return ft(kernel) + ft(user), ft(idle)


def main():
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq shouxin-server.exe",
                          "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
    pid = int([p.strip('"') for p in out.splitlines()[0].split('","')][1])
    cores = os.cpu_count() or 32
    print("PID=%d  逻辑核=%d  线程=19 → 理论上限 %.0f%%" % (pid, cores, 19 * 100 / cores))

    done = [None]

    def fire():
        body = json.dumps({
            "conversation": "帮我记一下：下周三之前把季度复盘发给李经理，另外她说明天下午三点开会。",
            "user_id": "cpu_probe3"}).encode()
        r = urllib.request.Request("http://127.0.0.1:3000/api/memory/add", data=body,
                                   headers={"Content-Type": "application/json"}, method="POST")
        with urllib.request.urlopen(r, timeout=300) as resp:
            done[0] = json.loads(resp.read().decode()).get("success")

    th = threading.Thread(target=fire)
    th.start()

    p_prev, p_t = proc_cpu(pid), time.time()
    kb, ki = sys_times()
    proc_s, sys_s = [], []
    for _ in range(60):
        time.sleep(2)
        p_cur, p_now = proc_cpu(pid), time.time()
        b2, i2 = sys_times()
        proc_pct = (p_cur - p_prev) / ((p_now - p_t) * cores) * 100
        db, di = b2 - kb, i2 - ki
        sys_pct = (db - di) / db * 100 if db > 0 else 0.0
        proc_s.append(proc_pct)
        sys_s.append(sys_pct)
        p_prev, p_t = p_cur, p_now
        kb, ki = b2, i2
        if done[0] is not None and len(proc_s) > 3:
            break
    th.join()

    busy = [(p, s) for p, s in zip(proc_s, sys_s) if p > 3]
    print("采样曲线（项目占整机 / 整机总占用）：")
    print("  " + "  ".join("%.0f/%.0f" % (p, s) for p, s in zip(proc_s, sys_s)))
    if busy:
        print("推理期间：项目均值 %.1f%%（峰值 %.1f%%），整机均值 %.1f%%"
              % (sum(p for p, _ in busy) / len(busy),
                 max(p for p, _ in busy),
                 sum(s for _, s in busy) / len(busy)))
    print("抽取成功：", done[0])


if __name__ == "__main__":
    main()
