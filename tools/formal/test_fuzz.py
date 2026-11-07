import argparse
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

from fuzz import cleanup_children, finish_case, main, positive, scenario, trace_status
from validate import TraceError


class CampaignChecks(unittest.TestCase):
    def test_campaign_limits_must_be_positive(self):
        self.assertEqual(positive("3"), 3)
        for value in ("0", "-1"):
            with self.assertRaises(argparse.ArgumentTypeError):
                positive(value)

    def test_seed_reproduces_generated_scenario(self):
        self.assertEqual(scenario(13), scenario(13))

    def test_baseline_can_start_before_requested_shutdown(self):
        case = scenario(0)
        self.assertEqual(len(case["services"]), 1)
        self.assertEqual(case["services"][0]["mode"], "sleep")
        self.assertTrue(case["signal_when_running"])
        self.assertFalse(case["escalate"])

    def test_cleanup_never_signals_a_reused_pid(self):
        with patch("fuzz.process_identity", return_value=(123, "new-start-time")), \
             patch("fuzz.os.kill") as kill:
            cleanup_children({456: "original-start-time"})
            kill.assert_not_called()

    def test_cleanup_targets_and_reaps_only_the_recorded_child(self):
        with patch("fuzz.process_identity", return_value=(123, "original-start-time")), \
             patch("fuzz.os.kill") as kill, \
             patch("fuzz.os.waitpid", return_value=(456, 0)) as wait:
            cleanup_children({456: "original-start-time"})
            self.assertEqual(kill.call_args.args[0], 456)
            self.assertEqual(wait.call_args.args[0], 456)

    def test_malformed_trace_cannot_skip_cleanup(self):
        with tempfile.TemporaryDirectory() as folder:
            trace = Path(folder) / "trace.ndjson"
            trace.write_text("not json\n")
            child = Mock()
            child.poll.return_value = 0
            with patch("fuzz.cleanup_children") as cleanup:
                with self.assertRaises(ValueError):
                    finish_case(child, trace, {456: "identity"})
                cleanup.assert_called_once_with({456: "identity"})

    def test_wait_failure_cannot_skip_cleanup(self):
        child = Mock()
        child.poll.return_value = 0
        child.wait.side_effect = RuntimeError("wait failed")
        with patch("fuzz.cleanup_children") as cleanup:
            with self.assertRaisesRegex(RuntimeError, "wait failed"):
                finish_case(child, Path("/nonexistent-formal-trace"), {})
            cleanup.assert_called_once_with({})

    def test_tooling_and_unmapped_tlc_failures_are_inconclusive(self):
        self.assertEqual(trace_status(TraceError("jar", stage="tooling")), "inconclusive")
        self.assertEqual(trace_status(TraceError("exit 13", stage="tlc")), "inconclusive")
        self.assertEqual(trace_status(TraceError("ambiguous", stage="uncertifiable")),
                         "inconclusive")
        for stage in ("parse", "project", "initial", "action"):
            self.assertEqual(trace_status(TraceError("bad", stage=stage)), "rejected")

    def test_campaign_manifest_separates_tooling_from_rejections(self):
        errors = [TraceError("TLC timeout", stage="tooling"),
                  TraceError("TLC rejected record 9", record=9, kind="step", stage="action")]
        with tempfile.TemporaryDirectory() as folder, \
             patch("fuzz.checker"), patch("fuzz.schedule_case"), \
             patch("fuzz.validate", side_effect=errors), \
             patch("sys.argv", ["fuzz.py", "schedule", "--count", "2", "--out",
                                str(Path(folder) / "campaign")]):
            self.assertEqual(main(), 1)
            manifest = json.loads((Path(folder) / "campaign" / "manifest.json").read_text())
        self.assertEqual([r["status"] for r in manifest["results"]], ["inconclusive", "rejected"])


if __name__ == "__main__":
    unittest.main()
