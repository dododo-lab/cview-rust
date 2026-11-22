#!/usr/bin/env python3
"""성능 회귀 게이트 — 기준 커밋과 지금 작업 트리를 같은 Mac 에서 번갈아 재고 비교한다.

최적화 감사(2026-11-23) 8.2 정량 검증 프로토콜 중 저장소에 있는 측정 예제를 자동으로 돌린다.

  python3 scripts/perf-gate.py                       # 기준 origin/main, 마이크로벤치만
  python3 scripts/perf-gate.py --base v0.2.15 --rounds 5
  python3 scripts/perf-gate.py --checks              # fmt·clippy·test·node --check 도
  python3 scripts/perf-gate.py --markdown out.md     # PR 본문에 붙일 표

- 기준 커밋은 임시 worktree 로 꺼내고, 두 쪽을 각자의 CARGO_TARGET_DIR 에서 release 로 빌드한다
  (공유 target 을 쓰면 다른 세션의 바이너리를 잴 수 있다).
- 측정은 기준·지금을 번갈아 `--rounds` 번 돌려 각 지표의 중앙값을 비교한다(열·부하 흐름이 한쪽에만 쏠리지 않게).
- 지금 쪽이 `--threshold`(기본 1.15 = 15%) 넘게 느리고 차이가 `--floor-ms`(기본 0.05ms) 보다 크면 실패(종료 코드 1).
- 기준 커밋에 없는 측정 예제는 지금 쪽 예제 파일을 기준 트리에 넣어 빌드해 본다(공개 API 가 같으면 비교된다).
  빌드가 안 되면 지금 쪽 값만 적고 비교하지 않는다.

외부 네트워크·서버 설정은 쓰지 않는다. 실앱 CPU·GPU·RSS(perf/tools/suite2.sh)와 화면 검사(tools/ui-regression)는
여기서 돌리지 않는다 — PR 체크리스트(.github/pull_request_template.md)를 따른다.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


@dataclass(frozen=True)
class Probe:
    """측정 예제 하나 — 크레이트·예제 이름·인자. 인자의 `{tmp}` 는 실행마다 새 임시 폴더."""

    key: str
    package: str
    example: str
    args: tuple[str, ...] = ()


PROBES: tuple[Probe, ...] = (
    Probe("chat-parse", "cview-chat", "parse_probe"),
    Probe("store", "cview-persistence", "perf_probe", ("{tmp}/store.sqlite", "20000")),
    Probe("image-cache", "cview-persistence", "image_cache_probe", ("3000",)),
)

# parse_probe: "frame of 10 msgs (10972 B): 5.84 µs/msg"
_FRAME = re.compile(r"^frame of\s+(\d+) msgs \(\d+ B\):\s+([0-9.]+)\s+µs/msg")
# perf_probe · image_cache_probe: "name   median    1.533ms  min    1.451ms"
_MEDIAN = re.compile(r"^(.+?)\s+median\s+([0-9.]+)ms\s+min\s+([0-9.]+)ms")


def parse_metrics(text: str) -> dict[str, float]:
    """측정 출력 → {지표 이름: 밀리초}. µs 는 밀리초로 바꾼다."""
    out: dict[str, float] = {}
    for raw in text.splitlines():
        line = raw.strip()
        m = _FRAME.match(line)
        if m:
            out[f"parse {int(m.group(1))}-msg frame (per msg)"] = float(m.group(2)) / 1000.0
            continue
        m = _MEDIAN.match(line)
        if m:
            out[m.group(1).strip()] = float(m.group(2))
    return out


@dataclass
class Verdict:
    name: str
    base: float | None
    head: float
    ratio: float | None
    regressed: bool


def compare(
    base: dict[str, list[float]],
    head: dict[str, list[float]],
    threshold: float,
    floor_ms: float,
) -> list[Verdict]:
    """지표별 중앙값 비교. 느려진 비율이 threshold 를 넘고 절대 차이가 floor_ms 를 넘을 때만 회귀."""
    verdicts = []
    for name, values in head.items():
        h = statistics.median(values)
        b_values = base.get(name)
        if not b_values:
            verdicts.append(Verdict(name, None, h, None, False))
            continue
        b = statistics.median(b_values)
        ratio = h / b if b > 0 else None
        regressed = ratio is not None and ratio > threshold and (h - b) > floor_ms
        verdicts.append(Verdict(name, b, h, ratio, regressed))
    return verdicts


def markdown(verdicts: list[Verdict], base_ref: str, rounds: int, threshold: float) -> str:
    lines = [
        f"성능 게이트 — 기준 `{base_ref}` 대비, 번갈아 {rounds}회 중앙값, 회귀 기준 ×{threshold:.2f}",
        "",
        "작을수록 빠름. 기준에 없던 측정 예제는 지금 쪽 파일을 기준 트리에 넣어 잰다.",
        "",
        "| 지표 | 기준 | 지금 | 비율 | 판정 |",
        "|---|---:|---:|---:|---|",
    ]
    for v in verdicts:
        b = "—" if v.base is None else f"{v.base:.4f} ms"
        r = "—" if v.ratio is None else f"×{v.ratio:.2f}"
        mark = "회귀" if v.regressed else ("기준 없음" if v.base is None else "통과")
        lines.append(f"| {v.name} | {b} | {v.head:.4f} ms | {r} | {mark} |")
    return "\n".join(lines) + "\n"


# ── 실행 ─────────────────────────────────────────


def run(
    cmd: list[str],
    cwd: Path,
    env: dict[str, str] | None = None,
    capture: bool = False,
    check: bool = True,
) -> str:
    shown = " ".join(cmd)
    print(f"$ ({cwd.name}) {shown}", file=sys.stderr, flush=True)
    result = subprocess.run(
        cmd,
        cwd=cwd,
        env={**os.environ, **(env or {})},
        text=True,
        stdout=subprocess.PIPE if capture else None,
    )
    if result.returncode != 0:
        if not check:
            return "\0failed"
        raise SystemExit(f"실패({result.returncode}): {shown}")
    return result.stdout or ""


def example_path(tree: Path, probe: Probe) -> Path:
    return tree / "crates" / probe.package / "examples" / f"{probe.example}.rs"


def has_example(tree: Path, probe: Probe) -> bool:
    return example_path(tree, probe).is_file()


def build(tree: Path, target: Path, probes: list[Probe], borrowed: frozenset[Probe] = frozenset()) -> list[Probe]:
    """빌드된 측정 예제 목록. `borrowed`(지금 쪽에서 빌려 넣은 예제)는 빌드가 안 되면 빼고, 나머지는 실패면 중단."""
    built = []
    for p in probes:
        out = run(
            ["cargo", "build", "--release", "-q", "-p", p.package, "--example", p.example],
            tree,
            {"CARGO_TARGET_DIR": str(target)},
            check=p not in borrowed,
        )
        if out == "\0failed":
            print(f"기준 쪽에서 {p.example} 를 빌드하지 못해 비교에서 뺍니다", file=sys.stderr)
            continue
        built.append(p)
    return built


def measure(target: Path, probe: Probe) -> dict[str, float]:
    exe = target / "release" / "examples" / probe.example
    with tempfile.TemporaryDirectory(prefix="cview-perf-gate-") as tmp:
        args = [a.replace("{tmp}", tmp) for a in probe.args]
        out = run([str(exe), *args], Path(tmp), capture=True)
    return {f"{probe.key}: {k}": v for k, v in parse_metrics(out).items()}


def static_checks(tree: Path) -> None:
    run(["cargo", "fmt", "--all", "--", "--check"], tree)
    run(["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"], tree)
    run(["cargo", "test", "--workspace"], tree)
    for js in sorted((tree / "app-ui" / "js").glob("*.js")):
        run(["node", "--check", str(js)], tree)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--base", default="origin/main", help="비교할 기준 커밋(기본 origin/main)")
    ap.add_argument("--rounds", type=int, default=3, help="번갈아 잴 횟수(기본 3)")
    ap.add_argument("--threshold", type=float, default=1.15, help="회귀로 볼 느려짐 비율(기본 1.15)")
    ap.add_argument("--floor-ms", type=float, default=0.05, help="이보다 작은 절대 차이는 무시(ms)")
    ap.add_argument("--checks", action="store_true", help="fmt·clippy·test·node --check 도 돌린다")
    ap.add_argument("--work-dir", type=Path, help="기준 worktree·target 을 둘 폴더(기본 임시 폴더)")
    ap.add_argument("--markdown", type=Path, help="결과 표를 이 파일에 쓴다")
    ap.add_argument("--json", type=Path, help="원자료(라운드별 값)를 이 파일에 쓴다")
    args = ap.parse_args(argv)

    if args.checks:
        static_checks(ROOT)

    work = args.work_dir or Path(tempfile.mkdtemp(prefix="cview-perf-gate-"))
    work.mkdir(parents=True, exist_ok=True)
    base_tree = work / "base"
    base_sha = run(["git", "rev-parse", "--verify", f"{args.base}^{{commit}}"], ROOT, capture=True).strip()
    if base_tree.exists():
        run(["git", "worktree", "remove", "--force", str(base_tree)], ROOT)
    run(["git", "worktree", "add", "--detach", str(base_tree), base_sha], ROOT)
    try:
        head_probes = [p for p in PROBES if has_example(ROOT, p)]
        borrowed = frozenset(p for p in head_probes if not has_example(base_tree, p))
        for p in borrowed:
            shutil.copyfile(example_path(ROOT, p), example_path(base_tree, p))
        base_probes = build(base_tree, work / "target-base", head_probes, borrowed)
        build(ROOT, work / "target-head", head_probes)

        base_vals: dict[str, list[float]] = {}
        head_vals: dict[str, list[float]] = {}
        for i in range(args.rounds):
            print(f"── 라운드 {i + 1}/{args.rounds}", file=sys.stderr, flush=True)
            for p in head_probes:
                if p in base_probes:
                    for k, v in measure(work / "target-base", p).items():
                        base_vals.setdefault(k, []).append(v)
                for k, v in measure(work / "target-head", p).items():
                    head_vals.setdefault(k, []).append(v)
    finally:
        run(["git", "worktree", "remove", "--force", str(base_tree)], ROOT)
        if not args.work_dir:
            shutil.rmtree(work, ignore_errors=True)

    verdicts = compare(base_vals, head_vals, args.threshold, args.floor_ms)
    table = markdown(verdicts, f"{args.base} ({base_sha[:7]})", args.rounds, args.threshold)
    print(table)
    if args.markdown:
        args.markdown.write_text(table, encoding="utf-8")
    if args.json:
        args.json.write_text(
            json.dumps({"base": base_sha, "baseValues": base_vals, "headValues": head_vals}, ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
    regressed = [v.name for v in verdicts if v.regressed]
    if regressed:
        print("회귀: " + ", ".join(regressed), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
