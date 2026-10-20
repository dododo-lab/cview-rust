"""measure.py <label> <seconds> <pids-file>  — 앱 + WebKit 도우미 프로세스 합산 CPU%·GPU·전력·깨움·메모리."""
import subprocess, sys, re, time, json
label, dur, pidfile = sys.argv[1], int(sys.argv[2]), sys.argv[3]
pids = [int(x) for x in open(pidfile).read().split()]
def cputime():
    out = subprocess.run(['ps', '-o', 'pid=,time=,rss=,comm=', '-p', ','.join(map(str, pids))], capture_output=True, text=True).stdout
    r = {}
    for line in out.strip().splitlines():
        pid, t, rss, comm = line.split(None, 3)
        m = re.match(r'(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)', t)
        d, h, mi, s = m.groups(); sec = (int(d or 0)*86400) + int(h or 0)*3600 + int(mi)*60 + float(s)
        r[int(pid)] = (sec, int(rss), comm.split('/')[-1])
    return r
def gpu():
    out = subprocess.run(['ioreg', '-l', '-w0'], capture_output=True, text=True).stdout
    tot = {}; cur = None
    for line in out.splitlines():
        m = re.search(r'"IOUserClientCreator" = "pid (\d+),', line)
        if m: cur = int(m.group(1)); continue
        if '"AppUsage"' in line and cur in pids:
            tot[cur] = tot.get(cur, 0) + sum(int(x) for x in re.findall(r'"accumulatedGPUTime"=(\d+)', line))
    return sum(tot.values())
a, g0, t0 = cputime(), gpu(), time.time()
args = ['top', '-l', '2', '-s', str(dur), '-stats', 'pid,cpu,power,idlew,mem']
for p in pids: args += ['-pid', str(p)]
top = subprocess.run(args, capture_output=True, text=True).stdout
b, g1, t1 = cputime(), gpu(), time.time()
el = t1 - t0
cpu = {p: (b[p][0] - a[p][0]) / el * 100 for p in b if p in a}
last = top.split('PID')[-1].strip().splitlines()[1:]
power = idlew = 0.0
for line in last:
    f = line.split()
    if len(f) >= 4 and f[0].isdigit():
        power += float(f[2].rstrip('+-')); idlew += float(f[3].rstrip('+-'))
res = {'label': label, 'secs': round(el, 1), 'cpu_pct': round(sum(cpu.values()), 1),
       'cpu_by': {b[p][2]+':'+str(p): round(v, 1) for p, v in cpu.items()},
       'gpu_ms_per_s': round((g1 - g0) / 1e6 / el, 2), 'energy_impact': round(power, 1), 'idle_wakeups_per_s': round(idlew / el, 1) if False else round(idlew, 1),
       'rss_mb': round(sum(v[1] for v in b.values()) / 1024, 0)}
print(json.dumps(res, ensure_ascii=False))
