#!/usr/bin/env python3
"""metal-bench 실행기 — 숨은 WKWebView(bench)로 player.html 의 GPU 경로를 바꿔 가며 CPU·GPU 시간을 잰다.

화면에 창을 띄우지 않고(창은 화면 밖, 앱은 활성화하지 않음) 사용자의 CView 도 건드리지 않는다.
재는 것: bench 와 그 뒤에 뜬 WebKit 도우미(WebContent·GPU)의
  · CPU % (ps 누적 CPU 시간 차이)
  · GPU % (ioreg AGX 클라이언트의 accumulatedGPUTime 차이 — gpu-sample.py 와 같은 방식)
  · 영상 프레임/초(requestVideoFrameCallback 수)와 실제로 쓰인 백엔드(webgpu·webgl·none)
⚠ 화면 밖 창은 WindowServer 가 합성하지 않으므로 backdrop-filter(화면 합성 단계 흐림) 비용은 여기서 잡히지 않는다.

사용: run.py <root: old/·new/·clip.mp4 가 있는 폴더> <bench 실행 파일> [반복=2] [측정 초=12]
"""
import http.server
import json
import os
import re
import subprocess
import sys
import threading
import time

ROOT, BENCH = sys.argv[1], sys.argv[2]
REPEAT = int(sys.argv[3]) if len(sys.argv) > 3 else 2
MEASURE = float(sys.argv[4]) if len(sys.argv) > 4 else 12.0
WARMUP = 4.0
PORT = 47811


class Range(http.server.SimpleHTTPRequestHandler):
    """범위 요청(영상 재생에 필요)과 CORS 를 받는 정적 서버."""

    def __init__(self, *a, **k):
        super().__init__(*a, directory=ROOT, **k)

    def end_headers(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Accept-Ranges", "bytes")
        super().end_headers()

    def do_GET(self):
        path = self.translate_path(self.path)
        rng = self.headers.get("Range")
        if not rng or not os.path.isfile(path):
            return super().do_GET()
        size = os.path.getsize(path)
        m = re.match(r"bytes=(\d*)-(\d*)", rng)
        start = int(m.group(1)) if m.group(1) else 0
        end = int(m.group(2)) if m.group(2) else size - 1
        end = min(end, size - 1)
        self.send_response(206)
        self.send_header("Content-Type", "video/mp4")
        self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.send_header("Content-Length", str(end - start + 1))
        self.end_headers()
        with open(path, "rb") as f:
            f.seek(start)
            self.wfile.write(f.read(end - start + 1))

    def log_message(self, *a):
        pass


def webkit_pids():
    out = subprocess.run(["ps", "-axo", "pid=,comm="], capture_output=True, text=True).stdout
    return {int(l.split(None, 1)[0]) for l in out.splitlines() if "WebKit" in l}


def cpu_seconds(pids):
    if not pids:
        return 0.0
    out = subprocess.run(["ps", "-o", "time=", "-p", ",".join(map(str, pids))], capture_output=True, text=True).stdout
    total = 0.0
    for t in out.split():
        days = 0
        if "-" in t:
            d, t = t.split("-", 1)
            days = int(d)
        parts = [float(p) for p in t.split(":")]
        sec = 0.0
        for p in parts:
            sec = sec * 60 + p
        total += sec + days * 86400
    return total


def gpu_ns(pids):
    out = subprocess.run(["ioreg", "-l", "-w", "0"], capture_output=True, text=True).stdout
    total, pid, ws = 0, None, 0
    for line in out.splitlines():
        m = re.search(r'"IOUserClientCreator" = "pid (\d+), ([^"]+)"', line)
        if m:
            pid = (int(m.group(1)), m.group(2))
            continue
        if pid and "accumulatedGPUTime" in line:
            v = sum(int(x) for x in re.findall(r'"accumulatedGPUTime"=(\d+)', line))
            if pid[0] in pids:
                total += v
            if pid[1] == "WindowServer":
                ws += v
    return total, ws


def run_once(label, page, views, w, h, mode):
    before = webkit_pids()
    video = f"http://127.0.0.1:{PORT}/clip.mp4"
    secs = WARMUP + MEASURE + 2
    p = subprocess.Popen([BENCH, ROOT, page, video, str(views), str(w), str(h), str(secs), mode],
                         stdout=subprocess.PIPE, text=True)
    ticks = {}
    errors = []

    def reader():
        for line in p.stdout:
            m = re.match(r"tick (\d+) frames=(\d+) backend=(\S+) secure=(\S+) gpu=(\S+)", line)
            if m:
                ticks[int(m.group(1))] = (int(m.group(2)), m.group(3), m.group(4), m.group(5))
            elif line.startswith("setup error"):
                errors.append(line.strip())

    threading.Thread(target=reader, daemon=True).start()
    time.sleep(WARMUP)
    pids = {p.pid} | (webkit_pids() - before)
    c0, (g0, w0), f0 = cpu_seconds(pids), gpu_ns(pids), sum(t[0] for t in ticks.values())
    time.sleep(MEASURE)
    c1, (g1, w1), f1 = cpu_seconds(pids), gpu_ns(pids), sum(t[0] for t in ticks.values())
    backends = sorted({t[1] for t in ticks.values()})
    sec = sorted({f"secure={t[2]} gpu={t[3]}" for t in ticks.values()})
    p.wait(timeout=30)
    r = {
        "label": label, "views": views,
        "cpu_pct": round((c1 - c0) / MEASURE * 100, 1),
        "gpu_pct": round((g1 - g0) / 1e9 / MEASURE * 100, 1),
        "windowserver_gpu_pct": round((w1 - w0) / 1e9 / MEASURE * 100, 1),
        "fps": round((f1 - f0) / MEASURE / views, 1),
        "backend": ",".join(backends), "context": ";".join(sec), "errors": errors,
    }
    print(json.dumps(r, ensure_ascii=False), flush=True)
    return r


SHARP = "setSharpen(true,1)"
UPSHARP = "setPictureEnhancement({auto:false,upscale:true}); setSharpen(true,1)"
OVERLAY = ("document.body.insertAdjacentHTML('beforeend','<div style=\"position:absolute;right:16px;top:74px;bottom:60px;"
           "width:340px;border-radius:16px;background:rgba(12,13,16,.5);%s\"></div>')")
SCENARIOS = [
    ("plain video", "new/player.html", 1, 1280, 720, ""),
    ("sharpen | old webgl", "old/player.html", 1, 1280, 720, SHARP),
    ("sharpen | new webgl", "new/player.html?gpu=webgl", 1, 1280, 720, SHARP),
    ("sharpen | new auto", "new/player.html", 1, 1280, 720, SHARP),
    ("upscale+sharpen | old webgl", "old/player.html", 1, 1280, 720, UPSHARP),
    ("upscale+sharpen | new webgl", "new/player.html?gpu=webgl", 1, 1280, 720, UPSHARP),
    ("upscale+sharpen | new auto", "new/player.html", 1, 1280, 720, UPSHARP),
    ("4 panes sharpen | old webgl", "old/player.html", 4, 640, 360, SHARP),
    ("4 panes sharpen | new webgl", "new/player.html?gpu=webgl", 4, 640, 360, SHARP),
    ("4 panes sharpen | new auto", "new/player.html", 4, 640, 360, SHARP),
    ("overlay blur", "new/player.html", 1, 1280, 720, OVERLAY % "backdrop-filter:blur(10px);-webkit-backdrop-filter:blur(10px)"),
    ("overlay no blur", "new/player.html", 1, 1280, 720, OVERLAY % "text-shadow:0 1px 2px rgba(0,0,0,.55)"),
]

if __name__ == "__main__":
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Range)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    only = os.environ.get("ONLY")
    results = []
    for rep in range(REPEAT):
        order = SCENARIOS if rep % 2 == 0 else list(reversed(SCENARIOS))
        for s in order:
            if only and only not in s[0]:
                continue
            results.append(run_once(*s))
    print("\n== 평균 ==")
    by = {}
    for r in results:
        by.setdefault(r["label"], []).append(r)
    for s in SCENARIOS:
        rs = by.get(s[0])
        if not rs:
            continue
        avg = lambda k: round(sum(x[k] for x in rs) / len(rs), 1)
        print(f"{s[0]:<32} CPU {avg('cpu_pct'):>6}%  GPU {avg('gpu_pct'):>6}%  WS {avg('windowserver_gpu_pct'):>5}%  "
              f"fps {avg('fps'):>5}  [{rs[0]['backend']}] {rs[0]['context']}")
    srv.shutdown()
