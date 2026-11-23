"""scripts/perf-gate.py 의 출력 해석·비교 규칙 검사(측정은 돌리지 않는다)."""
import importlib.util
import sys
import unittest
from pathlib import Path

_SPEC = importlib.util.spec_from_file_location(
    "perf_gate", Path(__file__).resolve().parents[3] / "scripts" / "perf-gate.py"
)
gate = importlib.util.module_from_spec(_SPEC)
sys.modules["perf_gate"] = gate  # dataclass 가 모듈을 찾는다
_SPEC.loader.exec_module(gate)


class ParseMetrics(unittest.TestCase):
    def test_parse_probe_lines_become_ms(self):
        out = "frame of  1 msgs (1214 B): 7.15 µs/msg\nframe of 10 msgs (10972 B): 5.84 µs/msg\n"
        m = gate.parse_metrics(out)
        self.assertAlmostEqual(m["parse 1-msg frame (per msg)"], 0.00715)
        self.assertAlmostEqual(m["parse 10-msg frame (per msg)"], 0.00584)

    def test_median_lines_and_noise(self):
        out = (
            "rows 20172\n"
            "aggregate_insights               median    2.533ms  min    2.401ms\n"
            "watch_report 7d                  median    0.120ms  min    0.100ms\n"
            "entries 3000\n"
        )
        m = gate.parse_metrics(out)
        self.assertEqual(m, {"aggregate_insights": 2.533, "watch_report 7d": 0.12})


class Compare(unittest.TestCase):
    def test_regression_needs_ratio_and_absolute_gap(self):
        base = {"a": [1.0, 1.0, 1.1], "tiny": [0.010, 0.010, 0.010], "ok": [2.0, 2.0, 2.0]}
        head = {"a": [1.3, 1.2, 1.25], "tiny": [0.030, 0.030, 0.030], "ok": [2.1, 2.2, 2.0], "new": [5.0]}
        v = {x.name: x for x in gate.compare(base, head, threshold=1.15, floor_ms=0.05)}
        self.assertTrue(v["a"].regressed)
        self.assertFalse(v["tiny"].regressed, "×3 이지만 0.02ms 차이는 잡음")
        self.assertFalse(v["ok"].regressed)
        self.assertIsNone(v["new"].base)
        self.assertFalse(v["new"].regressed, "기준에 없는 지표는 비교하지 않는다")

    def test_markdown_marks(self):
        vs = gate.compare({"a": [1.0]}, {"a": [2.0], "b": [1.0]}, 1.15, 0.05)
        md = gate.markdown(vs, "origin/main (abc1234)", 3, 1.15)
        self.assertIn("| a | 1.0000 ms | 2.0000 ms | ×2.00 | 회귀 |", md)
        self.assertIn("| b | — | 1.0000 ms | — | 기준 없음 |", md)


if __name__ == "__main__":
    unittest.main()
