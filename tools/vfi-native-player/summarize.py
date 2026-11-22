#!/usr/bin/env python3
"""vfi-player 결과 폴더의 *.json 을 마크다운 표로 정리한다.

사용법: python3 -I tools/vfi-native-player/summarize.py <결과 폴더>
"""
import json
import pathlib
import sys


def pct(v):
    return "-" if v is None else f"{v * 100:.1f}%"


def num(v, fmt="{:.1f}"):
    return "-" if v is None else fmt.format(v)


def main():
    if len(sys.argv) != 2:
        print(__doc__.strip())
        return 64
    folder = pathlib.Path(sys.argv[1])
    rows = []
    for path in sorted(folder.glob("*.json")):
        try:
            d = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if d.get("tool") != "vfi-player":
            continue
        c, r, ms = d["counts"], d["ratios"], d["ms"]
        proc = ms.get("proc", {})
        crit = d.get("criteria", {})
        c2 = [k for k in ("C2_on_time_ge_95pct", "C2_late_le_1pct", "C2_toggles_le_60_per_min") if k in crit]
        c2s = "-" if not c2 else ("통과" if all(crit[k] for k in c2) else "미달(" + ", ".join(k.split("_", 1)[1] for k in c2 if not crit[k]) + ")")
        c8 = [k for k in ("C8_memory_growth_le_50mb", "C8_latency_not_growing") if k in crit]
        c8s = "-" if not c8 else ("통과" if all(crit[k] for k in c8) else "미달")
        lat = d.get("latency_s", {}).get("median")
        mem = d.get("memory_mb", {}).get("growth_after_60s_excluding_instrument")
        rows.append([
            path.stem, d.get("source", ""), num(d.get("wall_s"), "{:.0f}"), num(d.get("counted_s"), "{:.0f}"),
            str(c.get("eligible", 0)), pct(r.get("on_time")), pct(r.get("late_of_attempted")),
            str(c.get("late_start", 0)), str(c.get("bypass", 0)), str(c.get("breaker_trips", 0)),
            num(r.get("toggles_per_counted_min")),
            f"{num(proc.get('p50'))}/{num(proc.get('p95'))}", num(ms.get("lookahead", {}).get("min"), "{:.0f}"),
            num(lat, "{:.2f}"), num(mem), str(c.get("order_violation", 0)), str(c.get("stale_dropped", 0)),
            "✓" if d.get("accounting_ok") else "✗", str(c.get("reconnects", 0)), c2s, c8s,
        ])
    head = ["실행", "소스", "초", "집계 초", "대상 짝", "제때", "늦음", "시작 못함", "우회", "열림", "전환/분",
            "VT p50/p95(ms)", "앞당김 최소(ms)", "지연(s)", "메모리 증가(MB)", "순서 위반", "버린 결과", "집계", "재연결", "C2", "C8"]
    print("| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
    for row in rows:
        print("| " + " | ".join(row) + " |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
