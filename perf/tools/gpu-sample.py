#!/usr/bin/env python3
"""CView(본 프로세스 + 그 뒤에 뜬 WebKit 도우미)의 GPU 사용 시간 비율을 잰다.

ioreg 의 AGX 사용자 클라이언트마다 붙은 AppUsage.accumulatedGPUTime(ns)을 두 번 읽어 차이를 낸다.
출력: 프로세스별 GPU 바쁜 비율(%)과 합계, 시스템 전체 Device Utilization %.
사용: gpu_sample.py [초=10] [반복=1]
"""
import re
import subprocess
import sys
import time

INTERVAL = float(sys.argv[1]) if len(sys.argv) > 1 else 10.0
REPEAT = int(sys.argv[2]) if len(sys.argv) > 2 else 1


def snapshot():
    out = subprocess.run(["ioreg", "-l", "-w", "0"], capture_output=True, text=True).stdout
    per = {}
    pid = None
    for line in out.splitlines():
        m = re.search(r'"IOUserClientCreator" = "pid (\d+), ([^"]+)"', line)
        if m:
            pid = (int(m.group(1)), m.group(2))
            continue
        if pid and "accumulatedGPUTime" in line:
            for v in re.findall(r'"accumulatedGPUTime"=(\d+)', line):
                per[pid] = per.get(pid, 0) + int(v)
    util = re.findall(r'"Device Utilization %"=(\d+)', out)
    return per, (int(util[0]) if util else None)


def cview_pids():
    ps = subprocess.run(["ps", "-axo", "pid=,lstart=,comm="], capture_output=True, text=True).stdout
    rows = []
    for line in ps.splitlines():
        parts = line.split(None, 6)
        if len(parts) < 7:
            continue
        pid = int(parts[0])
        start = time.mktime(time.strptime(" ".join(parts[1:6]), "%a %b %d %H:%M:%S %Y"))
        rows.append((pid, start, parts[6]))
    app = [r for r in rows if r[2].endswith("/cview-app")]
    if not app:
        return set(), None
    app_pid, app_start, _ = max(app, key=lambda r: r[1])
    pids = {app_pid}
    for pid, start, comm in rows:
        if "WebKit" in comm and start + 2 >= app_start:
            pids.add(pid)
    return pids, app_pid


for _ in range(REPEAT):
    pids, app_pid = cview_pids()
    a, _ = snapshot()
    time.sleep(INTERVAL)
    b, util = snapshot()
    total = 0.0
    lines = []
    for key, v in b.items():
        if key[0] not in pids:
            continue
        d = (v - a.get(key, 0)) / 1e9 / INTERVAL * 100.0
        if d < 0.05:
            continue
        total += d
        lines.append(f"  {key[1]}[{key[0]}] {d:.1f}%")
    print(f"{time.strftime('%H:%M:%S')} CView GPU {total:.1f}%  (시스템 Device {util}%)")
    for l in sorted(lines):
        print(l)
