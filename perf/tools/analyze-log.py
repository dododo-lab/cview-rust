#!/usr/bin/env python3
"""디버그 로그(debug.jsonl) → 성능 표(마크다운).

사용:
  python3 perf/tools/analyze-log.py <debug.jsonl> [--since 2026-10-20T11:30:00] [--until ...] [--label 이름]

앱에서 설정 → 성능·진단 → "디버그 로그"를 켜거나 CVIEW_DEBUG_LOG=1 로 실행하면 남는다.
위치: macOS ~/Library/Application Support/app.cview.rust/logs/debug.jsonl (Windows %APPDATA%\\app.cview.rust\\logs).
"""
import argparse
import collections
import json
import statistics
import sys


def pct(values, p):
    if not values:
        return None
    s = sorted(values)
    return s[min(len(s) - 1, int(p * len(s)))]


def fmt(v, d=1):
    if v is None:
        return "—"
    if isinstance(v, float):
        return f"{v:.{d}f}"
    return str(v)


def load(path, since, until):
    rows = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            ts = r.get("ts", "")
            if since and ts < since:
                continue
            if until and ts > until:
                continue
            rows.append(r)
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument("--since")
    ap.add_argument("--until")
    ap.add_argument("--label", default="")
    a = ap.parse_args()
    rows = load(a.log, a.since, a.until)
    if not rows:
        print("기록이 없습니다", file=sys.stderr)
        return 1
    by = collections.defaultdict(list)
    for r in rows:
        by[r["kind"]].append(r)
    out = [f"## {a.label or '디버그 로그 분석'}", "", f"구간: {rows[0]['ts']} ~ {rows[-1]['ts']} · 기록 {len(rows)}줄", ""]

    # 시작
    st = {k: by[k][0]["up_ms"] for k in ("startup.setup_begin", "startup.services_ready", "startup.setup_end") if by.get(k)}
    ui_st = by.get("ui.startup", [])
    flf = [r["ms"] for r in by.get("ui.first_live_frame", []) if r.get("ms") is not None]
    if st or ui_st or flf:
        out += ["### 시작", "| 항목 | ms |", "|---|---:|"]
        for k, v in st.items():
            out.append(f"| 프로세스 시작 → {k.split('.', 1)[1]} | {fmt(v)} |")
        for r in ui_st[:1]:
            out.append(f"| 페이지 로드 → 조작 가능 | {fmt(r.get('page_to_interactive_ms'))} |")
            out.append(f"| 페이지 로드 → DOMContentLoaded | {fmt(r.get('dom_content_loaded_ms'))} |")
        if flf:
            out.append(f"| 재생 요청 → 첫 라이브 프레임(중앙값, {len(flf)}회) | {fmt(statistics.median(flf))} |")
        out.append("")

    # 프로세스
    pr = by.get("process", [])
    if pr:
        cpu = [r["cpu_total"] for r in pr]
        rss = [r["rss_total_mb"] for r in pr]
        chat = [r.get("chat_msgs_per_s", 0) for r in pr]
        out += ["### 프로세스(10초 표본, 앱 + 웹뷰 도우미 합)", "| 지표 | 평균 | p95 | 최대 | 처음 → 끝 |", "|---|---:|---:|---:|---:|",
                f"| CPU % | {fmt(statistics.mean(cpu))} | {fmt(pct(cpu, .95))} | {fmt(max(cpu))} | |",
                f"| RSS MB | {fmt(statistics.mean(rss))} | {fmt(pct(rss, .95))} | {fmt(max(rss))} | {fmt(rss[0])} → {fmt(rss[-1])} |",
                f"| 채팅 메시지/s | {fmt(statistics.mean(chat))} | {fmt(pct(chat, .95))} | {fmt(max(chat))} | |", ""]
        per = collections.defaultdict(list)
        for r in pr:
            for p in r.get("procs", []):
                per[p["name"]].append(p)
        out += ["| 프로세스 | 평균 CPU % | 평균 RSS MB | 스레드 |", "|---|---:|---:|---:|"]
        for name, ps in sorted(per.items(), key=lambda kv: -statistics.mean(x["cpu"] for x in kv[1])):
            th = [x.get("threads") for x in ps if x.get("threads") is not None]
            out.append(f"| {name} | {fmt(statistics.mean(x['cpu'] for x in ps))} | {fmt(statistics.mean(x['rss_mb'] for x in ps))} | {fmt(max(th)) if th else '—'} |")
        out.append("")

    # 네트워크
    net = by.get("net.request", [])
    if net:
        minutes = collections.Counter(r["ts"][:16] for r in net if not r.get("cache_hit"))
        real = [r for r in net if not r.get("cache_hit")]
        errs = collections.Counter(r.get("error") or f"http {r.get('status')}" for r in real if r.get("error") or (r.get("status") or 200) >= 400)
        ms = [r["ms"] for r in real if r.get("ms") is not None]
        paths = collections.Counter(r["path"].split("/service/")[-1] for r in real)
        out += ["### 네트워크(API 요청)", "| 지표 | 값 |", "|---|---:|",
                f"| 실제 요청 | {len(real)} |", f"| 캐시 적중 | {sum(1 for r in net if r.get('cache_hit'))} |",
                f"| 분당 요청(평균 / 최대) | {fmt(statistics.mean(minutes.values()))} / {max(minutes.values())} |",
                f"| 걸린 시간 p50 / p95 ms | {fmt(pct(ms, .5))} / {fmt(pct(ms, .95))} |",
                f"| 실패 | {sum(errs.values())} ({', '.join(f'{k} {v}' for k, v in errs.most_common(4)) or '없음'}) |", "",
                "| 많이 부른 경로 | 횟수 |", "|---|---:|"]
        for p, n in paths.most_common(6):
            out.append(f"| `{p}` | {n} |")
        out.append("")

    # 화면 프레임
    fr = [r for r in by.get("ui.frames", []) if r.get("visible") and r.get("n")]
    if fr:
        pages = collections.defaultdict(list)
        for r in fr:
            pages[r.get("page") or "?"].append(r)
        out += ["### 화면(10초 묶음)", "| 화면 | 묶음 | p95 ms(평균) | 최대 ms | 긴 프레임(>50ms) | DOM 요소 | IPC/10s | 채팅 줄/10s |", "|---|---:|---:|---:|---:|---:|---:|---:|"]
        for page, rs in pages.items():
            out.append(f"| {page} | {len(rs)} | {fmt(statistics.mean(r['p95_ms'] for r in rs if r.get('p95_ms') is not None))} | {fmt(max(r.get('max_ms') or 0 for r in rs))} | "
                       f"{sum(r.get('long_50ms', 0) for r in rs)} | {max(r.get('dom_nodes', 0) for r in rs)} | {fmt(statistics.mean(r.get('ipc_events') or 0 for r in rs))} | {fmt(statistics.mean(r.get('chat_lines') or 0 for r in rs))} |")
        out.append("")

    # 플레이어
    pl = [p for r in by.get("ui.player", []) for p in r.get("players", [])]
    if pl:
        w = collections.defaultdict(list)
        for p in pl:
            w[p["where"]].append(p)
        out += ["### 플레이어(10초 표본)", "| 위치 | 표본 | 해상도 | 버퍼 s(평균) | 라이브 지연 s(평균) | 버퍼링 횟수 | 버퍼링 ms | 떨어진 프레임 증가 |", "|---|---:|---|---:|---:|---:|---:|---:|"]
        for where, ps in sorted(w.items()):
            lat = [p["latency_s"] for p in ps if p.get("latency_s") is not None]
            drop = [p["dropped"] for p in ps if p.get("dropped") is not None]
            heights = collections.Counter(p.get("height") for p in ps)
            out.append(f"| {where} | {len(ps)} | {heights.most_common(1)[0][0]}p | {fmt(statistics.mean(p.get('buffer_s') or 0 for p in ps), 2)} | "
                       f"{fmt(statistics.mean(lat), 2) if lat else '—'} | {sum(p.get('stalls', 0) for p in ps)} | {sum(p.get('stall_ms', 0) for p in ps)} | {(max(drop) - min(drop)) if drop else '—'} |")
        ev = collections.Counter((r.get("source"), r.get("type")) for r in by.get("player.event", []))
        if ev:
            out.append("")
            out.append("플레이어 신호: " + ", ".join(f"{s} {t} {n}" for (s, t), n in ev.most_common(8)))
        out.append("")

    cs = collections.Counter(r.get("state") for r in by.get("chat.state", []))
    if cs:
        out += ["### 채팅 연결", "상태 변화: " + ", ".join(f"{k} {v}" for k, v in cs.most_common()), ""]
    print("\n".join(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
