#!/usr/bin/env python3
"""백그라운드 지연 측정 분석 — run.sh 결과 폴더 하나를 읽어 구간별 표를 낸다.

입력: debug*.jsonl(player.latency), web.jsonl(웹 기준), events.txt(launch/hide/unhide/done), cpu.txt
출력: 마크다운 표(표준 출력) + <폴더>/series.csv(시각별 앱·웹 값)
"""
import json, sys, os, glob, statistics as st

d = sys.argv[1]
ev = {}
for line in open(os.path.join(d, 'events.txt')):
    p = line.split()
    if len(p) == 2 and p[0].isdigit():
        ev[p[1]] = int(p[0])
app = []
for f in sorted(glob.glob(os.path.join(d, 'debug*.jsonl'))):
    for line in open(f):
        try:
            o = json.loads(line)
        except Exception:
            continue
        if o.get('kind') == 'player.latency':
            app.append(o['data'])
app.sort(key=lambda x: x['t'])
web = []
if os.path.exists(os.path.join(d, 'web.jsonl')):
    for line in open(os.path.join(d, 'web.jsonl')):
        try:
            o = json.loads(line)
        except Exception:
            continue
        if o.get('wall') is not None and not o.get('paused'):
            web.append(o)

hide, unhide, done = ev.get('hide'), ev.get('unhide'), ev.get('done')
WARM = 60_000  # 시작 뒤 60초는 버린다(시작 자리 잡기)
phases = [
    ('전경', ev['launch'] + WARM, hide),
    ('숨김 전체', hide, unhide),
    ('숨김 1분 뒤~', hide + 60_000, unhide),
    ('복귀', unhide, done),
]


def q(xs, p):
    xs = sorted(xs)
    if not xs:
        return float('nan')
    k = (len(xs) - 1) * p
    lo = int(k)
    hi = min(lo + 1, len(xs) - 1)
    return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)


def sel(rows, a, b):
    return [r for r in rows if a <= r['t'] < b]


cpu = {}
if os.path.exists(os.path.join(d, 'cpu.txt')):
    for line in open(os.path.join(d, 'cpu.txt')):
        p = line.split(None, 4)
        if len(p) < 5:
            continue
        ts, pid, c, rss, comm = int(p[0]), p[1], float(p[2]), int(p[3]), p[4].strip()
        name = 'app' if 'cview-app' in comm else ('WebContent' if 'WebContent' in comm else ('GPU' if 'GPU' in comm else 'Net'))
        cpu.setdefault(ts, {}).setdefault(name, [0.0, 0])
        cpu[ts][name][0] += c
        cpu[ts][name][1] += rss

web_lat_fg = [r['latency'] for r in sel(web, *phases[0][1:]) if r.get('latency')]
web_ref = st.median(web_lat_fg) if web_lat_fg else float('nan')

wref = 0.0
print(f"### {os.path.basename(d)}\n")
print(f"표본: 앱 {len(app)}개, 웹 {len(web)}개. 웹 기준 지연(전경 구간 hls.latency 중앙값) = {web_ref:.2f}s\n")
print('| 구간 | 앱 지연 중앙(p5–p95) | 웹 지연 중앙 | 앱−웹 벽시계 지연 중앙 | 앱 지연이 웹 기준±0.5s 안 | 벽시계 차이 ±0.5s 안 | 벽시계 차이 p5–p95 | 끊김(waiting) | 1.03배 비율 | LL | 250ms 타이머 실제 간격 중앙 | CPU 앱+WebContent(%) |')
print('|---|---|---|---|---|---|---|---|---|---|---|---|')
for name, a, b in phases:
    if a is None or b is None:
        continue
    A = sel(app, a, b)
    W = sel(web, a, b)
    if not A:
        print(f'| {name} | (표본 없음) |')
        continue
    lat = [r['latency'] for r in A if r.get('latency') is not None]
    wl = [r['latency'] for r in W if r.get('latency') is not None]
    # 같은 초끼리 벽시계 지연 차이
    wi = 0
    diffs = []
    for r in A:
        if r.get('wall') is None:
            continue
        while wi + 1 < len(W) and abs(W[wi + 1]['t'] - r['t']) <= abs(W[wi]['t'] - r['t']):
            wi += 1
        if W and abs(W[wi]['t'] - r['t']) < 1500:
            diffs.append(r['wall'] - W[wi]['wall'])
    inside = sum(1 for x in lat if abs(x - web_ref) <= 0.5) / len(lat) if lat else 0
    waits = (A[-1]['waits'] - A[0]['waits']) if A else 0
    fast = sum(1 for r in A if r.get('rate', 1) > 1.001) / len(A)
    ll = sum(r.get('ll', 0) for r in A) / len(A)
    gaps = [r['gap'] for r in A if r.get('gap')]
    cp = [v.get('app', [0])[0] + v.get('WebContent', [0])[0] for ts, v in cpu.items() if a <= ts < b]
    print(f"| {name} | {st.median(lat):.2f} ({q(lat,.05):.2f}–{q(lat,.95):.2f}) | "
          f"{(st.median(wl) if wl else float('nan')):.2f} | {(st.median(diffs) if diffs else float('nan')):+.2f} | "
          f"{inside*100:.0f}% | {(sum(1 for x in diffs if abs(x - wref) <= 0.5) / len(diffs) * 100 if diffs else float('nan')):.0f}% | {q(diffs,.05)-wref:+.2f}–{q(diffs,.95)-wref:+.2f} | {waits} | {fast*100:.0f}% | {ll*100:.0f}% | {(st.median(gaps) if gaps else float('nan')):.0f}ms | "
          f"{(st.mean(cp) if cp else float('nan')):.1f} |")

# 복귀 점프: 복귀 앞뒤 15초의 벽시계 지연 변화·재생 위치 점프
if unhide:
    before = [r for r in app if unhide - 15_000 <= r['t'] < unhide and r.get('wall') is not None]
    after = [r for r in app if unhide <= r['t'] < unhide + 15_000 and r.get('wall') is not None]
    jumps = []
    seq = before + after
    for x, y in zip(seq, seq[1:]):
        dw = y['wall'] - x['wall']
        jumps.append(dw)
    big = [j for j in jumps if abs(j) > 0.3]
    wb = st.median([r['wall'] for r in before]) if before else float('nan')
    wa = st.median([r['wall'] for r in after]) if after else float('nan')
    wa_waits = (after[-1]['waits'] - before[0]['waits']) if before and after else 0
    print(f"\n복귀 앞뒤 15초: 벽시계 지연 {wb:.2f}s → {wa:.2f}s, 1초 사이 0.3s 넘는 변화 {len(big)}회 {['%+.2f' % j for j in big]}, 끊김 {wa_waits}회")

with open(os.path.join(d, 'series.csv'), 'w') as f:
    f.write('t_s,phase,hidden,latency,target,wall,ahead,rate,ll,waits,gap\n')
    t0 = ev['launch']
    for r in app:
        ph = 'fg' if r['t'] < (hide or 1e20) else ('bg' if r['t'] < (unhide or 1e20) else 'ret')
        f.write(f"{(r['t']-t0)/1000:.1f},{ph},{r.get('hidden')},{r.get('latency')},{r.get('target')},{r.get('wall')},{r.get('ahead')},{r.get('rate')},{r.get('ll')},{r.get('waits')},{r.get('gap')}\n")
