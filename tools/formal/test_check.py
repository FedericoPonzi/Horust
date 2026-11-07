import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import check


ROOT = Path(__file__).resolve().parents[2]


class CheckEntrypoint(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.artifacts = Path(tempfile.mkdtemp(prefix="horust-check-tests-"))
        cls.trace = cls.artifacts / "accepted.ndjson"
        subprocess.run(
            [str(ROOT / "target/debug/horust"), "--uds-folder-path", str(cls.artifacts),
             "--", "/bin/sleep 1"],
            env=dict(os.environ, HORUST_FORMAL_TRACE=str(cls.trace)),
            timeout=20, check=True, capture_output=True,
        )
        if not cls.trace.is_file():
            raise RuntimeError("target/debug/horust lacks formal-trace; run "
                               "cargo build -p horust --features formal-trace")

    def run_main(self, *argv):
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = check.main(list(argv))
        return code, stdout.getvalue() + stderr.getvalue()

    def test_matrix_covers_every_model_config(self):
        names = check.configs()
        self.assertIn("HorustCycle", names)
        self.assertIn("HorustShutdownCycle", names)
        self.assertIn("SupervisorTwo", names)
        self.assertNotIn("TraceValidation", names)
        self.assertEqual(len(names), len(list((ROOT / "specs/configs").glob("*.cfg"))) - 1)
        self.assertEqual(check.module_for("ExtendedTerm"), "ProcessExtended.tla")
        self.assertEqual(check.module_for("ProcessStartup"), "Process.tla")
        self.assertEqual(check.module_for("HorustFanIn"), "Horust.tla")
        self.assertEqual(check.module_for("SupervisorTwo"), "SupervisorMC.tla")

    def test_expected_negative_exit_handling(self):
        violated = "Error: Temporal property StartupProgress was violated."
        self.assertEqual(check.classify("HorustCycle", 13, violated)["status"], "ok")
        self.assertEqual(check.classify("HorustCycle", 0, "No error has been found")["status"],
                         "unexpected")
        self.assertEqual(check.classify("HorustTwo", 13, violated)["status"], "unexpected")
        self.assertEqual(check.classify("HorustTwo", 0, "No error has been found.")["status"], "ok")
        self.assertEqual(check.classify("HorustTwo", 0, "")["status"], "unexpected")
        shutdown = "Error: Temporal property ShutdownProgress was violated."
        self.assertEqual(check.classify("HorustShutdownCycle", 13, shutdown)["status"], "ok")
        self.assertEqual(check.classify("HorustShutdownCycle", 13, violated)["status"],
                         "unexpected")

    def test_models_run_cycle_negative_and_positive_config(self):
        out = self.artifacts / "models"
        code, text = self.run_main("models", "--only", "HorustCycle", "ProcessStartup",
                                   "--out", str(out))
        self.assertEqual(code, 0, text)
        result = json.loads((out / "result.json").read_text())
        exits = {r["name"]: (r["expected_exit"], r["exit"], r["status"]) for r in result["models"]}
        self.assertEqual(exits["HorustCycle"], (13, 13, "ok"))
        self.assertEqual(exits["ProcessStartup"], (0, 0, "ok"))
        self.assertTrue((out / "HorustCycle" / "tlc.log").is_file())

    def test_models_refuse_existing_output(self):
        with self.assertRaises(FileExistsError):
            self.run_main("models", "--only", "ProcessStartup", "--out", str(self.artifacts))

    def test_validate_mode_accepts(self):
        code, text = self.run_main("validate", str(self.trace), "--out",
                                   str(self.artifacts / "validated"))
        self.assertEqual(code, 0, text)
        self.assertEqual(json.loads((self.artifacts / "validated" / "result.json").read_text())
                         ["status"], "accepted")

    def test_minimize_cli_fails_visibly_on_inapplicable_trace(self):
        code, text = self.run_main("minimize", str(self.trace), "--out",
                                   str(self.artifacts / "minimize-accepted"))
        self.assertNotEqual(code, 0)
        self.assertIn("FAILED", text)
        self.assertNotIn("passed", text.lower())
        self.assertNotIn("REJECTED", text)
        garbage = self.artifacts / "garbage.ndjson"
        garbage.write_text("{oops\n")
        code, text = self.run_main("minimize", str(garbage), "--out",
                                   str(self.artifacts / "minimize-garbage"))
        self.assertNotEqual(code, 0)
        self.assertIn("FAILED", text)

    def test_tooling_failure_prints_error_not_rejected(self):
        bogus = self.artifacts / "bogus.jar"
        bogus.write_text("not tlc")
        with patch.dict(os.environ, TLC_JAR=str(bogus)):
            code, text = self.run_main("validate", str(self.trace), "--out",
                                       str(self.artifacts / "validate-bad-jar"))
        self.assertEqual(code, 1)
        self.assertIn("ERROR", text)
        self.assertNotIn("REJECTED", text)

    def test_replay_delegates_single_seed(self):
        self.assertEqual(check.fuzz_command(["replay", "schedule", "5", "--depth", "40"]),
                         [sys.executable, str(ROOT / "tools/formal/fuzz.py"), "schedule",
                          "--seed", "5", "--count", "1", "--depth", "40"])
        self.assertEqual(check.fuzz_command(["fuzz", "process", "--count", "3"]),
                         [sys.executable, str(ROOT / "tools/formal/fuzz.py"), "process",
                          "--count", "3"])


if __name__ == "__main__":
    unittest.main()
