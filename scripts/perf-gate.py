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
    """측정 예제 하나 — 크레이트·예제 이름·인자·필수 지표. 인자의 `{tmp}` 는 실행마다 새 임시 폴더."""

    key: str
    package: str
    example: str
    args: tuple[str, ...] = ()
    required_metrics: tuple[str, ...] = ()


PROBES: tuple[Probe, ...] = (
    Probe(
        "chat-parse",
        "cview-chat",
        "parse_probe",
        required_metrics=(
            "parse 1-msg frame (per msg)",
            "parse 10-msg frame (per msg)",
        ),
    ),
    Probe(
        "store",
        "cview-persistence",
        "perf_probe",
        ("{tmp}/store.sqlite", "20000"),
        required_metrics=(
            "aggregate_insights",
            "recent_watch_history_rows(60)",
            "all_watch_history_rows",
            "watch_report today",
            "watch_report 7d",
            "watch_report 30d",
            "watch_report all",
            "fetch_favorite_items",
            "fetch_recent_items(20)",
            "fetch_recent_vod_playback(40)",
            "watch_history_fingerprint",
            "table_row_count",
        ),
    ),
    Probe(
        "image-cache",
        "cview-persistence",
        "image_cache_probe",
        ("3000",),
        required_metrics=(
            "get 60 hits (same screen)",
            "put 20 new",
        ),
    ),
)

# parse_probe: "frame of 10 msgs (10972 B): 5.84 µs/msg"
_FRAME = re.compile(r"^frame of\s+(\d+) msgs \(\d+ B\):\s+([0-9.]+)\s+µs/msg")
# perf_probe · image_cache_probe: "name   median    1.533ms  min    1.451ms"
_MEDIAN = re.compile(r"^(.+?)\s+median\s+([0-9.]+)ms\s+min\s+([0-9.]+)ms")


class ProbeMeasurementError(RuntimeError):
    """실행한 측정 예제에서 지표를 읽지 못했거나 필수 지표가 빠졌을 때 발생한다."""

    def __init__(self, probe: Probe, detail: str) -> None:
        self.probe = probe
        self.detail = detail
        super().__init__(f"{probe.key} ({probe.example}): {detail}")


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


def collect_probe_metrics(
    probe: Probe,
    text: str,
    *,
    require_expected: bool = True,
) -> dict[str, float]:
    """예제 출력에서 지표를 읽고, 0개이거나 필수 지표가 빠졌으면 예제 이름과 함께 오류를 낸다."""
    raw = parse_metrics(text)
    if not raw:
        raise ProbeMeasurementError(probe, "측정된 지표가 0개입니다")
    if require_expected and probe.required_metrics:
        missing = [name for name in probe.required_metrics if name not in raw]
        if missing:
            raise ProbeMeasurementError(probe, f"필수 지표 누락: {', '.join(missing)}")
    return {f"{probe.key}: {k}": v for k, v in raw.items()}


@dataclass
class Verdict:
    name: str
    base: float | None
    head: float | None
    ratio: float | None
    regressed: bool
    missing: bool = False


def compare(
    base: dict[str, list[float]],
    head: dict[str, list[float]],
    threshold: float,
    floor_ms: float,
) -> list[Verdict]:
    """지표별 중앙값 비교. 기준에 있던 지표가 현재 측정에서 빠지면 missing=True 로 기록한다."""
    verdicts: list[Verdict] = []
    seen: set[str] = set()
    for name, values in head.items():
        if not values:
            continue
        seen.add(name)
        h = statistics.median(values)
        b_values = base.get(name)
        if not b_values:
            verdicts.append(Verdict(name, None, h, None, False))
            continue
        b = statistics.median(b_values)
        ratio = h / b if b > 0 else None
        regressed = ratio is not None and ratio > threshold and (h - b) > floor_ms
        verdicts.append(Verdict(name, b, h, ratio, regressed))
    for name, b_values in base.items():
        if name in seen or not b_values:
            continue
        b = statistics.median(b_values)
        verdicts.append(Verdict(name, b, None, None, False, missing=True))
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
        h = "—" if v.head is None else f"{v.head:.4f} ms"
        r = "—" if v.ratio is None else f"×{v.ratio:.2f}"
        if v.missing:
            mark = "누락"
        elif v.regressed:
            mark = "회귀"
        elif v.base is None:
            mark = "기준 없음"
        else:
            mark = "통과"
        lines.append(f"| {v.name} | {b} | {h} | {r} | {mark} |")
    return "\n".join(lines) + "\n"


def gate_failures(
    verdicts: list[Verdict],
    probe_errors: list[str] | None = None,
) -> list[str]:
    """실패 사유 목록. 비어 있으면 게이트 통과(종료 코드 0)."""
    failures: list[str] = []
    if probe_errors:
        failures.append("측정 예제 오류: " + "; ".join(probe_errors))
    missing = [v.name for v in verdicts if v.missing]
    if missing:
        failures.append("지표 누락: " + ", ".join(missing))
    regressed = [v.name for v in verdicts if v.regressed]
    if regressed:
        failures.append("회귀: " + ", ".join(regressed))
    if not verdicts and not failures:
        failures.append("측정된 지표가 없습니다")
    return failures


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


def measure(target: Path, probe: Probe, *, require_expected: bool = True) -> dict[str, float]:
    exe = target / "release" / "examples" / probe.example
    with tempfile.TemporaryDirectory(prefix="cview-perf-gate-") as tmp:
        args = [a.replace("{tmp}", tmp) for a in probe.args]
        out = run([str(exe), *args], Path(tmp), capture=True)
    return collect_probe_metrics(probe, out, require_expected=require_expected)


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
        probe_errors: list[str] = []
        seen_errors: set[str] = set()
        if not head_probes:
            probe_errors.append("실행할 측정 예제가 없습니다")
        for i in range(args.rounds):
            print(f"── 라운드 {i + 1}/{args.rounds}", file=sys.stderr, flush=True)
            for p in head_probes:
                if p in base_probes:
                    try:
                        for k, v in measure(work / "target-base", p, require_expected=False).items():
                            base_vals.setdefault(k, []).append(v)
                    except ProbeMeasurementError as exc:
                        msg = f"기준 {exc}"
                        if msg not in seen_errors:
                            seen_errors.add(msg)
                            probe_errors.append(msg)
                try:
                    for k, v in measure(work / "target-head", p, require_expected=True).items():
                        head_vals.setdefault(k, []).append(v)
                except ProbeMeasurementError as exc:
                    msg = str(exc)
                    if msg not in seen_errors:
                        seen_errors.add(msg)
                        probe_errors.append(msg)
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
            json.dumps(
                {
                    "base": base_sha,
                    "baseValues": base_vals,
                    "headValues": head_vals,
                    "probeErrors": probe_errors,
                },
                ensure_ascii=False,
                indent=2,
            ),
            encoding="utf-8",
        )
    failures = gate_failures(verdicts, probe_errors)
    if failures:
        for line in failures:
            print(line, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
