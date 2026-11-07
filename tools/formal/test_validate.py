import copy
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

from fuzz import trace_status
from validate import MAX_ATTEMPT, TraceError, bundle, diagnose, encode_attempts, minimize, project, run_tlc, tla, validate


ROOT = Path(__file__).resolve().parents[2]


class TraceChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.artifacts = Path(tempfile.mkdtemp(prefix="horust-trace-tests-"))
        cls.trace = cls.artifacts / "healthy.ndjson"
        env = dict(os.environ, HORUST_FORMAL_TRACE=str(cls.trace))
        subprocess.run(
            [str(ROOT / "target/debug/horust"), "--uds-folder-path", str(cls.artifacts),
             "--", "/bin/sleep 2"],
            env=env, timeout=20, check=True, capture_output=True,
        )
        if not cls.trace.is_file():
            raise RuntimeError("target/debug/horust lacks formal-trace; run "
                               "cargo build -p horust --features formal-trace")
        cls.records = [json.loads(line) for line in cls.trace.read_text().splitlines()]
        worker_dir = cls.artifacts / "worker"
        worker_dir.mkdir()
        (worker_dir / "svc.toml").write_text(
            'command = "/bin/sleep 2"\n[healthiness]\nhealthy-after = "100ms"\n'
        )
        worker_trace = cls.artifacts / "worker.ndjson"
        env["HORUST_FORMAL_TRACE"] = str(worker_trace)
        subprocess.run(
            [str(ROOT / "target/debug/horust"), "--uds-folder-path", str(worker_dir),
             "--services-path", str(worker_dir)],
            env=env, timeout=20, check=True, capture_output=True,
        )
        cls.worker_records = [json.loads(line) for line in worker_trace.read_text().splitlines()]

    def project_mutation(self, records):
        for number, record in enumerate(records):
            record["seq"] = number
        return project(records)

    def check_records(self, records, name):
        path = self.artifacts / f"{name}.ndjson"
        path.write_text("".join(json.dumps(record) + "\n" for record in records))
        return validate(path, self.artifacts / name)

    def test_real_complete_trace_is_accepted(self):
        result = self.check_records(self.records, "accepted")
        self.assertGreater(result["steps"], 0)
        self.assertIn("Running", result["states"])
        self.assertTrue(result["complete"])
        self.assertNotIn("/bin/sleep", self.trace.read_text())

    def test_owner_cannot_invent_a_noop_run_after_valid_trace(self):
        records = copy.deepcopy(self.records)
        owner = next(r["data"]["receiver"] for r in records if r["kind"] == "initial")
        index = max(i for i, r in enumerate(records) if r["kind"] == "generate")
        state = copy.deepcopy(records[index]["data"]["state"])
        payload = dict(kind="Run", svc="s0", target="", pid=0, code=0,
                       good=False, mode="", attempt_id=0)
        records[index:index] = [
            dict(version=4, seq=0, kind="send",
                 data=dict(message=1000000, sender=owner, event=payload)),
            dict(version=4, seq=0, kind="dispatch",
                 data=dict(message=1000000, receivers=[owner])),
            dict(version=4, seq=0, kind="receive",
                 data=dict(message=1000000, receiver=owner)),
            dict(version=4, seq=0, kind="step",
                 data=dict(cause=1000000, before=state, after=state,
                           input=payload, outputs=[])),
        ]
        for number, record in enumerate(records):
            record["seq"] = number
        with self.assertRaisesRegex(TraceError, "owner publication"):
            self.check_records(records, "invented-owner-run")

    def test_owner_publication_marker_is_required(self):
        records = copy.deepcopy(self.records)
        del records[next(i for i, r in enumerate(records) if r["kind"] == "publish")]
        with self.assertRaisesRegex(TraceError, "owner publication"):
            self.project_mutation(records)

    def test_owner_publication_cannot_assert_its_own_sources(self):
        for mutation in ("extra", "missing", "reordered"):
            with self.subTest(mutation=mutation):
                records = copy.deepcopy(self.records)
                batch = next(r["data"]["events"] for r in records
                             if r["kind"] == "publish" and len(r["data"]["events"]) > 1)
                if mutation == "extra":
                    batch.append(dict(kind="Run", svc="s0", target="", pid=0, code=0,
                                      good=False, mode="", attempt_id=0))
                elif mutation == "missing":
                    batch.pop()
                else:
                    batch[0], batch[-1] = batch[-1], batch[0]
                with self.assertRaisesRegex(TraceError, "batch differs from ordered sources"):
                    self.project_mutation(records)

    def test_owner_cannot_publish_without_generation(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "initial")
        records.insert(index + 1, dict(version=4, seq=0, kind="publish", data=dict(events=[])))
        with self.assertRaisesRegex(TraceError, "no command generation"):
            self.project_mutation(records)

    def test_duplicate_owner_publication_is_rejected(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "publish"
                     and r["data"]["events"])
        records.insert(index + 1, copy.deepcopy(records[index]))
        with self.assertRaisesRegex(TraceError, "overlapping owner publication"):
            self.project_mutation(records)

    def test_duplicate_owner_send_is_rejected(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "send"
                     and r["data"]["event"]["kind"] == "Run")
        duplicate = copy.deepcopy(records[index])
        duplicate["data"]["message"] = 1000000
        records.insert(index + 1, duplicate)
        with self.assertRaisesRegex(TraceError, "owner publication"):
            self.project_mutation(records)

    def test_missing_ordinary_owner_output_cannot_complete(self):
        records = copy.deepcopy(self.records)
        sent = next(r for r in reversed(records) if r["kind"] == "send"
                    and r["data"]["event"]["kind"] == "StatusChanged")
        mid = sent["data"]["message"]
        records = [r for r in records if not (
            r["kind"] in {"send", "dispatch", "receive"} and r["data"]["message"] == mid)
            and not (r["kind"] == "step" and r["data"].get("cause") == mid)]
        with self.assertRaisesRegex(TraceError, "incomplete owner publication"):
            self.project_mutation(records)

    def test_owner_sends_must_follow_batch_order(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "publish"
                     and len(r["data"]["events"]) > 1)
        owner = next(r["data"]["receiver"] for r in records if r["kind"] == "initial")
        sends = [r for r in records[index + 1:] if r["kind"] == "send"
                 and r["data"]["sender"] == owner][:2]
        sends[0]["data"]["event"], sends[1]["data"]["event"] = (
            sends[1]["data"]["event"], sends[0]["data"]["event"])
        with self.assertRaisesRegex(TraceError, "owner publication"):
            self.project_mutation(records)

    def test_owner_cannot_handle_input_before_draining_publication(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "publish"
                     and r["data"]["events"])
        step = copy.deepcopy(next(r for r in records[index + 1:] if r["kind"] == "step"))
        records.insert(index + 1, step)
        with self.assertRaisesRegex(TraceError, "owner step before publication batch drained"):
            self.project_mutation(records)

    def test_final_owner_publication_must_be_fully_sent(self):
        for mutation in ("missing-marker", "missing-send", "duplicate-marker"):
            with self.subTest(mutation=mutation):
                records = copy.deepcopy(self.records)
                index = max(i for i, r in enumerate(records) if r["kind"] == "publish")
                if mutation == "missing-marker":
                    del records[index]
                elif mutation == "duplicate-marker":
                    records.insert(index, copy.deepcopy(records[index]))
                else:
                    sent = next(r for r in records[index + 1:] if r["kind"] == "send")
                    mid = sent["data"]["message"]
                    records = [r for r in records if not (
                        r["kind"] in {"send", "dispatch", "receive"} and r["data"]["message"] == mid)]
                with self.assertRaisesRegex(TraceError, "owner publication"):
                    self.project_mutation(records)

    def test_stale_running_is_consumed_without_promoting_unready_service(self):
        config, initial, actions, _ = project(self.records)
        position = next(i for i, a in enumerate(actions) if a["kind"] == "step"
                        and a["input"]["kind"] == "StatusUpdate"
                        and a["input"]["target"] == "Running")
        queued = copy.deepcopy(actions[position])
        service = queued["input"]["svc"]
        unready = copy.deepcopy(queued["before"])
        unready["services"][service]["hc"] = 1
        failed_check = dict(
            kind="step", before=queued["before"], after=unready, outputs=[],
            input=dict(kind="HealthCheck", svc=service, target="", pid=0, code=0,
                       good=False, mode="", attempt_id=unready["services"][service]["attempt_id"]),
            signal_count=0, record=-1,
        )
        queued.update(before=unready, after=copy.deepcopy(unready), outputs=[])
        for name, promote, expected in (("stale-noop", False, 0), ("stale-promote", True, 12)):
            held = copy.deepcopy(queued)
            if promote:
                held["after"]["services"][service]["status"] = "Running"
                held["outputs"] = actions[position]["outputs"]
            out = self.artifacts / name
            out.mkdir()
            bundle(out, config, initial, actions[:position] + [failed_check, held])
            code, _ = run_tlc(out)
            self.assertEqual(code, expected)

    def test_registration_requires_consumed_preparation(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "preparation_take")
        del records[index]
        with self.assertRaisesRegex(TraceError, "preparation|handoff"):
            self.project_mutation(records)

    def test_forged_exit_notification_requires_local_accounting(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "initial")
        owner = records[index]["data"]["receiver"]
        records.insert(index + 1, dict(version=4, seq=0, kind="send",
                       data=dict(message=1000000, sender=owner,
                                 event=dict(kind="ServiceExited", svc="s0", target="", pid=0,
                                            code=0, good=False, mode="", attempt_id=0))))
        with self.assertRaisesRegex(TraceError, "exit notification has no locally accounted reap"):
            self.project_mutation(records)

    def test_exit_notification_must_match_accounted_exit_code(self):
        records = copy.deepcopy(self.records)
        sent = next(r for r in records if r["kind"] == "send"
                    and r["data"]["event"]["kind"] == "ServiceExited")
        sent["data"]["event"]["code"] += 1
        with self.assertRaisesRegex(TraceError, "exit notification has no locally accounted reap"):
            self.project_mutation(records)

    def test_missing_exit_notification_cannot_complete(self):
        records = copy.deepcopy(self.records)
        sent = next(r for r in records if r["kind"] == "send"
                    and r["data"]["event"]["kind"] == "ServiceExited")
        mid = sent["data"]["message"]
        records = [
            r for r in records
            if not (r["kind"] in {"send", "dispatch", "receive"} and r["data"]["message"] == mid)
            and not (r["kind"] == "step" and r["data"].get("cause") == mid)
        ]
        with self.assertRaisesRegex(TraceError, "owner publication"):
            self.project_mutation(records)

    def test_reaped_exit_cannot_be_applied_twice(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "step"
                     and r["data"]["input"]["kind"] == "ReapedExit")
        records.insert(index + 1, copy.deepcopy(records[index]))
        with self.assertRaisesRegex(TraceError, "observed reap"):
            self.project_mutation(records)

    def test_local_exit_pid_must_match_observed_reap(self):
        records = copy.deepcopy(self.records)
        step = next(r for r in records if r["kind"] == "step"
                    and r["data"]["input"]["kind"] == "ReapedExit")
        step["data"]["input"]["pid"] += 1
        with self.assertRaisesRegex(TraceError, "observed reap"):
            self.project_mutation(records)

    def test_preparation_result_cannot_change_run_origin(self):
        records = copy.deepcopy(self.records)
        result = next(r for r in records if r["kind"] == "preparation_result")
        result["data"]["cause"] += 1000000
        with self.assertRaisesRegex(TraceError, "preparation result"):
            self.project_mutation(records)

    def test_worker_cannot_create_a_child_before_owner_handoff(self):
        records = copy.deepcopy(self.records)
        birth = next(i for i, r in enumerate(records) if r["kind"] == "birth")
        record = records.pop(birth)
        take = next(i for i, r in enumerate(records) if r["kind"] == "preparation_take")
        records.insert(take, record)
        with self.assertRaisesRegex(TraceError, "preparation|handoff"):
            self.project_mutation(records)

    def test_duplicate_owner_registration_is_rejected(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "step"
                     and r["data"]["input"]["kind"] == "SpawnRegistered")
        records.insert(index + 1, copy.deepcopy(records[index]))
        with self.assertRaisesRegex(TraceError, "duplicate|registration|preparation"):
            self.project_mutation(records)

    def test_invalid_initial_ownership_cannot_remove_all_initial_states(self):
        records = copy.deepcopy(self.records)
        initial = next(r for r in records if r["kind"] == "initial")
        initial["data"]["state"]["pid_map"] = [[123, "s0"]]
        with self.assertRaisesRegex(TraceError, "TLC"):
            self.check_records(records, "invalid-initial-ownership")

    def test_missing_record_fails(self):
        records = copy.deepcopy(self.records)
        del records[3]
        with self.assertRaisesRegex(TraceError, "sequence"):
            self.check_records(records, "missing")

    def test_reordered_record_fails(self):
        records = copy.deepcopy(self.records)
        records[3], records[4] = records[4], records[3]
        with self.assertRaises(TraceError):
            self.check_records(records, "reordered")

    def test_truncated_trace_fails(self):
        with self.assertRaisesRegex(TraceError, "completion"):
            self.check_records(self.records[:-1], "truncated")

    def test_wrong_version_fails(self):
        records = copy.deepcopy(self.records)
        records[0]["version"] = 99
        with self.assertRaisesRegex(TraceError, "version"):
            self.check_records(records, "version")

    def test_bad_initial_state_fails_in_tlc(self):
        records = copy.deepcopy(self.records)
        initial = next(r for r in records if r["kind"] == "initial")
        initial["data"]["state"]["phase"] = "Forceful"
        with self.assertRaisesRegex(TraceError, "TLC"):
            self.check_records(records, "initial")

    def test_illegal_transition_fails_in_tlc(self):
        records = copy.deepcopy(self.records)
        step = next(r for r in records if r["kind"] == "step"
                    and r["data"]["input"]["kind"] == "SpawnRegistered")
        service = step["data"]["input"]["svc"]
        step["data"]["after"]["services"][service]["status"] = "Running"
        with self.assertRaisesRegex(TraceError, "TLC"):
            self.check_records(records, "illegal")

    def test_pid_ownership_mismatch_fails_in_tlc(self):
        records = copy.deepcopy(self.records)
        step = next(r for r in records if r["kind"] == "step"
                    and r["data"]["input"]["kind"] == "SpawnRegistered")
        step["data"]["after"]["pid_map"] = []
        with self.assertRaisesRegex(TraceError, "TLC"):
            self.check_records(records, "ownership")

    def test_shutdown_downgrade_fails_in_tlc(self):
        records = copy.deepcopy(self.records)
        step = next(r for r in records if r["kind"] == "step")
        step["data"]["before"]["phase"] = "Forceful"
        step["data"]["after"]["phase"] = "Graceful"
        with self.assertRaisesRegex(TraceError, "TLC"):
            self.check_records(records, "downgrade")

    def test_unknown_record_cannot_be_skipped(self):
        records = copy.deepcopy(self.records)
        records[3]["kind"] = "unrecognized_action"
        with self.assertRaisesRegex(TraceError, "unknown"):
            self.check_records(records, "unknown")

    def test_worker_trace_is_accepted(self):
        self.check_records(self.worker_records, "worker-accepted")

    def test_signal_shutdown_and_final_publication_are_accepted(self):
        case = self.artifacts / "signal"
        case.mkdir()
        trace = case / "trace.ndjson"
        child = subprocess.Popen(
            [str(ROOT / "target/debug/horust"), "--uds-folder-path", str(case),
             "--", "/bin/sleep 8"],
            env=dict(os.environ, HORUST_FORMAL_TRACE=str(trace)),
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        try:
            deadline = time.monotonic() + 6
            while time.monotonic() < deadline:
                if trace.exists() and any(
                    r["kind"] == "step" and any(
                        p["status"] == "Running" for p in r["data"]["after"]["services"].values())
                    for r in (json.loads(line) for line in trace.read_text().splitlines(keepends=True)
                              if line.endswith("\n"))
                ):
                    child.send_signal(signal.SIGTERM)
                    break
                time.sleep(0.01)
            else:
                self.fail("service did not reach Running before signal deadline")
            child.communicate(timeout=10)
            self.assertEqual(child.returncode, 0)
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate(timeout=3)
        records = [json.loads(line) for line in trace.read_text().splitlines()]
        self.assertTrue(any(r["kind"] == "step" and r["data"]["input"]["kind"] == "Shutdown"
                            and r["data"]["cause"] == 0 for r in records))
        self.check_records(records, "signal-accepted")
        for mutation in ("missing", "duplicate"):
            with self.subTest(mutation=mutation):
                changed = copy.deepcopy(records)
                index = next(i for i, r in enumerate(changed) if r["kind"] == "step"
                             and r["data"]["input"]["kind"] == "Shutdown"
                             and r["data"]["cause"] == 0)
                sent = next(i for i in range(index + 1, len(changed))
                            if changed[i]["kind"] == "send")
                if mutation == "duplicate":
                    extra = copy.deepcopy(changed[sent])
                    extra["data"]["message"] = 1000000
                    changed.insert(sent + 1, extra)
                else:
                    mid = changed[sent]["data"]["message"]
                    changed = [r for r in changed if not (
                        r["kind"] in {"send", "dispatch", "receive"} and r["data"]["message"] == mid)
                        and not (r["kind"] == "step" and r["data"].get("cause") == mid)]
                with self.assertRaisesRegex(TraceError, "owner publication|signal"):
                    self.project_mutation(changed)

    def test_multiple_local_reaps_publish_all_outputs_before_raw_notifications(self):
        case = self.artifacts / "multi-reap"
        case.mkdir()
        for name in ("first", "second", "third"):
            (case / f"{name}.toml").write_text('command = "/bin/sleep 0.1"\n')
        trace = case / "trace.ndjson"
        subprocess.run(
            [str(ROOT / "target/debug/horust"), "--uds-folder-path", str(case),
             "--services-path", str(case)],
            env=dict(os.environ, HORUST_FORMAL_TRACE=str(trace)),
            timeout=15, check=True, capture_output=True,
        )
        records = [json.loads(line) for line in trace.read_text().splitlines()]
        self.check_records(records, "multi-reap-accepted")
        batch = next(r["data"]["events"] for r in records if r["kind"] == "publish"
                     and sum(e["kind"] == "ServiceExited" for e in r["data"]["events"]) > 1)
        outputs = [i for i, e in enumerate(batch) if e["kind"] == "StatusChanged"]
        notifications = [i for i, e in enumerate(batch) if e["kind"] == "ServiceExited"]
        self.assertLess(max(outputs), min(notifications))
        batch[outputs[-1]], batch[notifications[0]] = (
            batch[notifications[0]], batch[outputs[-1]])
        with self.assertRaisesRegex(TraceError, "batch differs from ordered sources"):
            self.project_mutation(records)

    def test_immediate_exit_is_reaped_with_registered_owner(self):
        case = self.artifacts / "immediate"
        case.mkdir()
        trace = case / "trace.ndjson"
        subprocess.run(
            [str(ROOT / "target/debug/horust"), "--uds-folder-path", str(case), "--", "/bin/true"],
            env=dict(os.environ, HORUST_FORMAL_TRACE=str(trace)),
            timeout=15, check=True, capture_output=True,
        )
        result = validate(trace, case / "check")
        self.assertEqual(result["actions"]["SpawnRegistered"], 1)
        self.assertEqual(result["actions"]["ReapedExit"], 1)
        self.assertTrue(result["complete"])

    def test_never_healthy_service_terminates_and_replays(self):
        case = self.artifacts / "never-healthy"
        case.mkdir()
        (case / "svc.toml").write_text(
            'command = "/bin/sleep 8"\n'
            '[healthiness]\nfile-path = "' + str(case / "missing") + '"\nmax-failed = 0\n'
            '[failure]\nstrategy = "shutdown"\n'
        )
        trace = case / "trace.ndjson"
        subprocess.run(
            [str(ROOT / "target/debug/horust"), "--uds-folder-path", str(case),
             "--services-path", str(case)],
            env=dict(os.environ, HORUST_FORMAL_TRACE=str(trace)),
            timeout=15, check=True, capture_output=True,
        )
        result = validate(trace, case / "check")
        self.assertIn("InKilling", result["states"])
        self.assertNotIn("Running", result["states"])
        self.assertIn("FinishedFailed", result["states"])
        self.assertTrue(result["complete"])

    def test_fifo_is_checked_after_renumbering(self):
        records = copy.deepcopy(self.records)
        dispatch = next(i for i, r in enumerate(records) if r["kind"] == "dispatch")
        receive = next(i for i, r in enumerate(records) if r["kind"] == "receive")
        records[dispatch], records[receive] = records[receive], records[dispatch]
        with self.assertRaisesRegex(TraceError, "FIFO"):
            self.project_mutation(records)

    def test_duplicate_receipt_is_rejected(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "receive")
        records.insert(index, copy.deepcopy(records[index]))
        with self.assertRaisesRegex(TraceError, "FIFO"):
            self.project_mutation(records)

    def test_dropped_owner_step_is_rejected_after_renumbering(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "step")
        del records[index]
        with self.assertRaisesRegex(TraceError, "owner publication batch differs"):
            self.project_mutation(records)

    def test_unreaped_child_is_rejected(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "reap")
        del records[index]
        with self.assertRaisesRegex(TraceError, "unreaped|observed reap"):
            self.project_mutation(records)

    def test_ambiguous_birth_is_inconclusive_not_successful(self):
        records = copy.deepcopy(self.records)
        records.insert(-1, dict(version=4, seq=0, kind="ambiguous_birth", data={}))
        with self.assertRaisesRegex(TraceError, "ambiguous") as caught:
            self.project_mutation(records)
        self.assertEqual(caught.exception.stage, "uncertifiable")
        for number, record in enumerate(records):
            record["seq"] = number
        with self.assertRaises(TraceError) as caught:
            self.check_records(records, "ambiguous")
        self.assertEqual(trace_status(caught.exception), "inconclusive")
        result = self.result_of("ambiguous")
        self.assertEqual((result["status"], result["stage"], result["kind"]),
                         ("error", "uncertifiable", "ambiguous_birth"))

    def test_deadline_uses_recorded_context(self):
        records = copy.deepcopy(self.records)
        initial = next(i for i, r in enumerate(records) if r["kind"] == "initial")
        records.insert(initial + 1, dict(version=4, seq=0, kind="deadline",
                       data=dict(svc="s0", due=True, forced=False, wait_secs=5, elapsed_secs=None)))
        with self.assertRaisesRegex(TraceError, "deadline"):
            self.project_mutation(records)

    def test_unexplained_shutdown_cannot_be_invented(self):
        records = copy.deepcopy(self.records)
        index = next(i for i, r in enumerate(records) if r["kind"] == "supervisor_finished")
        before = records[index]["data"]["state"]
        after = copy.deepcopy(before)
        after["phase"] = "Graceful"
        records.insert(index, dict(version=4, seq=0, kind="step",
                       data=dict(before=before, after=after, cause=0, outputs=[],
                                 input=dict(kind="Shutdown", svc="", target="", pid=0, code=0,
                                            good=False, mode="Graceful", attempt_id=0))))
        with self.assertRaisesRegex(TraceError, "no pending signals"):
            self.project_mutation(records)

    def test_health_cannot_precede_healthy_after(self):
        records = copy.deepcopy(self.worker_records)
        health = next(r for r in records if r["kind"] == "health_result" and r["data"]["good"])
        health["data"]["elapsed_ms"] = 0
        with self.assertRaisesRegex(TraceError, "before healthy-after"):
            self.project_mutation(records)

    def test_finished_spawn_actor_cannot_send(self):
        records = copy.deepcopy(self.records)
        done = next(r for r in records if r["kind"] == "spawn_done")
        index = next(i for i, r in enumerate(records) if r["kind"] == "send"
                     and r["data"]["event"]["kind"] == "PidChanged")
        records[index]["data"]["actor"] = done["data"]["request"]
        with self.assertRaisesRegex(TraceError, "send after spawn_done"):
            self.project_mutation(records)

    def test_worker_cannot_retag_a_result_as_current(self):
        records = copy.deepcopy(self.worker_records)
        sent = next(r for r in records if r["kind"] == "send"
                    and r["data"]["event"]["kind"] == "HealthCheck")
        sent["data"]["event"]["attempt_id"] += 1
        with self.assertRaisesRegex(TraceError, "changed its worker attempt ID"):
            self.project_mutation(records)

    def test_immediate_health_cannot_retag_its_start(self):
        records = copy.deepcopy(self.records)
        sent = next(r for r in records if r["kind"] == "send"
                    and r["data"]["event"]["kind"] == "HealthCheck")
        sent["data"]["event"]["attempt_id"] += 1
        with self.assertRaisesRegex(TraceError, "changed its start attempt ID"):
            self.project_mutation(records)

    def test_immediate_path_cannot_publish_unhealthy(self):
        records = copy.deepcopy(self.records)
        sent = next(r for r in records if r["kind"] == "send"
                    and r["data"]["event"]["kind"] == "HealthCheck")
        sent["data"]["event"]["good"] = False
        with self.assertRaisesRegex(TraceError, "immediate health must be Healthy"):
            self.project_mutation(records)

    def test_health_observation_must_keep_worker_attempt(self):
        records = copy.deepcopy(self.worker_records)
        result = next(r for r in records if r["kind"] == "health_result")
        result["data"]["attempt_id"] += 1
        with self.assertRaisesRegex(TraceError, "health observation changed"):
            self.project_mutation(records)

    def test_health_start_tag_must_match_registration(self):
        records = copy.deepcopy(self.records)
        sent = next(r for r in records if r["kind"] == "send"
                    and r["data"]["event"]["kind"] == "StartHealthCheck")
        sent["data"]["event"]["attempt_id"] += 1
        with self.assertRaisesRegex(TraceError, "health start has no matching registration"):
            self.project_mutation(records)

    def test_worker_start_must_keep_the_supplied_attempt(self):
        records = copy.deepcopy(self.worker_records)
        start = next(r for r in records if r["kind"] == "worker_start")
        start["data"]["attempt_id"] += 1
        with self.assertRaisesRegex(TraceError, "worker start changed its attempt ID"):
            self.project_mutation(records)

    def test_exhausted_attempt_cannot_request_preparation(self):
        records = copy.deepcopy(self.records)
        initial = next(r for r in records if r["kind"] == "initial")
        initial["data"]["state"]["services"]["s0"]["attempt_id"] = MAX_ATTEMPT
        with self.assertRaisesRegex(TraceError, "ineligible/exhausted attempt"):
            self.project_mutation(records)

    def check_rule_expressions(self, name, cases, expressions, policy=None):
        config, initial, _, _ = project(self.records)
        if policy is not None:
            config = copy.deepcopy(config)
            config["s0"].update(policy)
        out = self.artifacts / name
        out.mkdir()
        encoding = bundle(out, config, initial, [], extra_attempt_values=cases)
        checks = expressions(lambda value: tla(encode_attempts(value, encoding)))
        module = out / "TraceValidation.tla"
        text = module.read_text()
        extra = (
            "\nVARIABLE rule_ok\n"
            "RuleInit == index = 1 /\\ snapshot = Initial /\\ valid = TRUE\n"
            "            /\\ rule_ok = (" + "\n /\\ ".join(checks) + ")\n"
            "RuleNext == UNCHANGED <<index, snapshot, valid, rule_ok>>\n"
            "RuleAccepted == rule_ok\n"
        )
        module.write_text(text[:text.rfind("\n====")] + extra + "\n" + "=" * 77 + "\n")
        (out / "TraceValidation.cfg").write_text(
            "INIT RuleInit\nNEXT RuleNext\nINVARIANT RuleAccepted\n"
        )
        code, log = run_tlc(out)
        self.assertEqual(code, 0, log)

    def test_shutdown_restarts_are_noops_and_running_controls_remain_eligible(self):
        _, initial, _, _ = project(self.records)
        cases = []
        for status in ("Success", "Failed"):
            for phase in ("Running", "Graceful", "Forceful"):
                before = copy.deepcopy(initial)
                before["phase"] = phase
                before["services"]["s0"].update(status=status, retries=1, attempt_id=2)
                inp = dict(kind="StatusUpdate", svc="s0", target="Initial", pid=0, code=0,
                           good=False, mode="", attempt_id=0)
                after = copy.deepcopy(before)
                outputs = []
                if phase == "Running":
                    after["services"]["s0"]["status"] = "Initial"
                    outputs = [dict(inp, kind="StatusChanged")]
                step = dict(kind="step", before=before, after=after, input=inp,
                            outputs=outputs, signal_count=0, record=-1)
                invalid = copy.deepcopy(step)
                if phase == "Running":
                    invalid["after"] = copy.deepcopy(before)
                    invalid["outputs"] = []
                else:
                    invalid["after"]["services"]["s0"]["status"] = "Initial"
                    invalid["outputs"] = [dict(inp, kind="StatusChanged")]
                cases.append((before, step, invalid))
        self.check_rule_expressions(
            "shutdown-restart-rules", cases,
            lambda render: [check for before, step, invalid in cases for check in (
                f"SnapshotOK({render(before)})",
                f"StepOK({render(before)}, {render(step)})",
                f"~StepOK({render(before)}, {render(invalid)})",
            )],
            policy=dict(strategy="Always", attempts=3),
        )

    def test_late_runs_do_not_allocate_attempts_or_prepare_services(self):
        _, initial, _, _ = project(self.records)
        cases = []
        for phase in ("Running", "Graceful", "Forceful"):
            for attempt in (7, MAX_ATTEMPT - 1):
                before = copy.deepcopy(initial)
                before["phase"] = phase
                before["services"]["s0"].update(attempt_id=attempt, hc=2, retries=2)
                inp = dict(kind="Run", svc="s0", target="", pid=0, code=0,
                           good=False, mode="", attempt_id=0)
                admitted = copy.deepcopy(before)
                admitted["services"]["s0"].update(
                    status="Starting", hc=-1, attempt_id=attempt + 1
                )
                start = dict(kind="step", before=before, after=admitted, input=inp,
                             outputs=[dict(inp, kind="StatusChanged", target="Starting")],
                             signal_count=0, record=-1)
                ignored = dict(start, after=copy.deepcopy(before), outputs=[])
                cases.append((before, start if phase == "Running" else ignored,
                              ignored if phase == "Running" else start))
        self.check_rule_expressions(
            "late-run-rules", cases,
            lambda render: [check for before, valid, invalid in cases for check in (
                f"SnapshotOK({render(before)})",
                f"StepOK({render(before)}, {render(valid)})",
                f"~StepOK({render(before)}, {render(invalid)})",
            )],
        )

    def test_mismatched_health_is_noop_but_current_results_still_apply(self):
        _, initial, _, _ = project(self.records)
        before = copy.deepcopy(initial)
        before["services"]["s0"].update(status="Started", pid=11, attempt_id=2, retries=3)
        before["pid_map"] = frozenset({(11, "s0")})
        cases = []
        for good in (False, True):
            inp = dict(kind="HealthCheck", svc="s0", target="", pid=0, code=0,
                       good=good, mode="", attempt_id=1)
            stale = dict(kind="step", before=before, after=before, input=inp,
                         outputs=[], signal_count=0, record=-1)
            applied = copy.deepcopy(before)
            applied["services"]["s0"]["hc"] = 0 if good else 1
            bad = dict(stale, after=applied)
            current = dict(bad, input=dict(inp, attempt_id=2))
            cases.append((before, stale, bad, current))
        self.check_rule_expressions(
            "health-attempt-rules", cases,
            lambda render: [check for before, stale, bad, current in cases for check in (
                f"SnapshotOK({render(before)})",
                f"StepOK({render(before)}, {render(stale)})",
                f"~StepOK({render(before)}, {render(bad)})",
                f"StepOK({render(before)}, {render(current)})",
            )],
        )

    def test_attempt_exhaustion_preserves_counters_and_shutdown_direction(self):
        _, initial, _, _ = project(self.records)
        cases = []
        for phase in ("Running", "Graceful", "Forceful"):
            before = copy.deepcopy(initial)
            before["phase"] = phase
            before["services"]["s0"].update(attempt_id=MAX_ATTEMPT, hc=2, retries=3)
            after = copy.deepcopy(before)
            after["phase"] = "Forceful" if phase == "Forceful" else "Graceful"
            after["services"]["s0"]["status"] = "FinishedFailed"
            inp = dict(kind="Run", svc="s0", target="", pid=0, code=0,
                       good=False, mode="", attempt_id=0)
            outputs = [
                dict(inp, kind="StatusChanged", target="FinishedFailed"),
                dict(inp, kind="Shutdown", svc="", mode=after["phase"]),
            ]
            step = dict(kind="step", before=before, after=after, input=inp,
                        outputs=outputs, signal_count=0, record=-1)
            wrapped = copy.deepcopy(step)
            wrapped["after"]["services"]["s0"]["attempt_id"] = 0
            cases.append((before, step, wrapped))
        before = copy.deepcopy(initial)
        before["services"]["s0"].update(attempt_id=MAX_ATTEMPT - 1, hc=2, retries=3)
        after = copy.deepcopy(before)
        after["services"]["s0"].update(status="Starting", hc=-1, attempt_id=MAX_ATTEMPT)
        inp = dict(kind="Run", svc="s0", target="", pid=0, code=0,
                   good=False, mode="", attempt_id=0)
        step = dict(kind="step", before=before, after=after, input=inp,
                    outputs=[dict(inp, kind="StatusChanged", target="Starting")],
                    signal_count=0, record=-1)
        wrapped = copy.deepcopy(step)
        wrapped["after"]["services"]["s0"]["attempt_id"] = 0
        cases.append((before, step, wrapped))
        self.check_rule_expressions(
            "attempt-exhaustion-rules", cases,
            lambda render: [check for before, step, wrapped in cases for check in (
                f"SnapshotOK({render(before)})",
                f"StepOK({render(before)}, {render(step)})",
                f"~StepOK({render(before)}, {render(wrapped)})",
            )],
        )

    def test_attempt_projection_cannot_hide_a_skipped_increment(self):
        config, initial, _, _ = project(self.records)
        after = copy.deepcopy(initial)
        after["services"]["s0"].update(status="Starting", attempt_id=7)
        inp = dict(kind="Run", svc="s0", target="", pid=0, code=0,
                   good=False, mode="", attempt_id=0)
        step = dict(kind="step", before=initial, after=after, input=inp,
                    outputs=[dict(inp, kind="StatusChanged", target="Starting")],
                    signal_count=0, record=-1)
        out = self.artifacts / "skipped-attempt"
        out.mkdir()
        bundle(out, config, initial, [step])
        code, _ = run_tlc(out)
        self.assertEqual(code, 12)

    def result_of(self, name):
        return json.loads((self.artifacts / name / "result.json").read_text())

    def illegal_records(self):
        records = copy.deepcopy(self.records)
        step = next(r for r in records if r["kind"] == "step"
                    and r["data"]["input"]["kind"] == "SpawnRegistered")
        step["data"]["after"]["services"][step["data"]["input"]["svc"]]["status"] = "Running"
        return records, step["seq"]

    def test_counterexample_index_maps_to_previous_action(self):
        actions = [dict(kind="generate", record=7), dict(kind="step", record=9,
                   input=dict(kind="PidChanged"))]
        log = "Error: Invariant Accepted is violated.\nState 1:\n/\\ index = 1\n" \
              "State 3:\n/\\ index = 3\n"
        found = diagnose(12, log, actions, 2)
        self.assertEqual((found["stage"], found["action"], found["record"], found["kind"],
                          found["input_kind"]), ("action", 1, 9, "step", "PidChanged"))
        initial = diagnose(12, "Error: Invariant Accepted is violated.\n/\\ index = 1\n",
                           actions, 2)
        self.assertEqual((initial["stage"], initial["record"], initial["kind"]),
                         ("initial", 2, "initial"))
        self.assertIsNone(diagnose(0, "No error has been found", actions, 2))
        self.assertEqual(diagnose(13, "Temporal properties were violated", actions, 2)["stage"], "tlc")

    def test_tlc_rejection_reports_record_kind_and_artifacts(self):
        records, seq = self.illegal_records()
        with self.assertRaises(TraceError) as caught:
            self.check_records(records, "diagnosed")
        self.assertEqual((caught.exception.record, caught.exception.kind), (seq, "step"))
        self.assertIn(f"record {seq}", str(caught.exception))
        result = self.result_of("diagnosed")
        self.assertEqual((result["status"], result["stage"], result["record"], result["kind"],
                          result["input_kind"]), ("rejected", "action", seq, "step", "SpawnRegistered"))
        self.assertGreater(result["coverage"]["steps"], result["accepted_prefix"]["steps"])
        self.assertEqual((self.artifacts / "diagnosed" / "input.ndjson").read_bytes(),
                         (self.artifacts / "diagnosed.ndjson").read_bytes())

    def test_initial_invalidity_is_distinct(self):
        records = copy.deepcopy(self.records)
        initial = next(r for r in records if r["kind"] == "initial")
        initial["data"]["state"]["phase"] = "Forceful"
        with self.assertRaises(TraceError) as caught:
            self.check_records(records, "initial-diagnosed")
        self.assertEqual((caught.exception.record, caught.exception.kind),
                         (initial["seq"], "initial"))
        self.assertEqual(self.result_of("initial-diagnosed")["stage"], "initial")
        with self.assertRaisesRegex(TraceError, "initial"):
            path = self.artifacts / "initial-diagnosed.ndjson"
            minimize(path, self.artifacts / "initial-minimized")

    def test_parser_failure_writes_result(self):
        path = self.artifacts / "garbage.ndjson"
        lines = self.trace.read_text().splitlines(keepends=True)
        path.write_text("".join(lines[:2]) + "{not json\n" + "".join(lines[3:]))
        with self.assertRaises(TraceError) as caught:
            validate(path, self.artifacts / "garbage")
        self.assertEqual(caught.exception.record, 2)
        result = self.result_of("garbage")
        self.assertEqual((result["status"], result["stage"], result["record"]),
                         ("rejected", "parse", 2))
        self.assertIn("JSON", result["reason"])
        self.assertEqual((self.artifacts / "garbage" / "input.ndjson").read_bytes(),
                         path.read_bytes())

    def test_projection_failure_reports_record(self):
        records = copy.deepcopy(self.records)
        records[0]["version"] = 99
        with self.assertRaises(TraceError) as caught:
            self.check_records(records, "projection-diagnosed")
        self.assertEqual((caught.exception.record, caught.exception.kind), (0, "header"))
        result = self.result_of("projection-diagnosed")
        self.assertEqual((result["stage"], result["record"], result["kind"]), ("project", 0, "header"))

    def test_minimize_reruns_smaller_prefix_with_same_violation(self):
        records, seq = self.illegal_records()
        path = self.artifacts / "to-minimize.ndjson"
        path.write_text("".join(json.dumps(record) + "\n" for record in records))
        out = self.artifacts / "minimized"
        result = minimize(path, out)
        self.assertEqual((result["status"], result["record"], result["kind"]),
                         ("reproduced", seq, "step"))
        self.assertLess(result["minimized_actions"], result["full_actions"])
        self.assertEqual(result["minimized_actions"], result["action"] + 1)
        self.assertIn("not a minimal raw causal trace", result["certification"])
        self.assertEqual((out / "full" / "input.ndjson").read_bytes(), path.read_bytes())
        data = (out / "minimized" / "TraceData.tla").read_text()
        self.assertLess(len(data), len((out / "full" / "TraceData.tla").read_text()))
        self.assertIn("Invariant Accepted is violated", (out / "minimized" / "tlc.log").read_text())
        self.assertEqual(json.loads((out / "minimize.json").read_text())["record"], seq)

    def test_minimize_refuses_accepted_and_incomplete_traces(self):
        path = self.artifacts / "minimize-accepted.ndjson"
        path.write_text(self.trace.read_text())
        with self.assertRaisesRegex(TraceError, "accepted"):
            minimize(path, self.artifacts / "minimize-accepted")
        failed = json.loads((self.artifacts / "minimize-accepted" / "minimize.json").read_text())
        self.assertEqual(failed["status"], "failed")
        path = self.artifacts / "minimize-truncated.ndjson"
        path.write_text("".join(self.trace.read_text().splitlines(keepends=True)[:-1]))
        with self.assertRaisesRegex(TraceError, "completion"):
            minimize(path, self.artifacts / "minimize-truncated")

    def test_unmappable_tlc_failure_is_error_not_rejection(self):
        with patch("validate.run_tlc", return_value=(13, "Temporal property violated")):
            with self.assertRaises(TraceError) as caught:
                self.check_records(self.records, "unmappable")
        self.assertEqual(caught.exception.stage, "tlc")
        result = self.result_of("unmappable")
        self.assertEqual((result["status"], result["stage"], result["record"]), ("error", "tlc", None))

    def test_wrong_jar_is_tooling_error(self):
        bogus = self.artifacts / "bogus.jar"
        bogus.write_text("not tlc")
        with patch.dict(os.environ, TLC_JAR=str(bogus)):
            with self.assertRaisesRegex(TraceError, "checksum") as caught:
                self.check_records(self.records, "wrong-jar")
        self.assertEqual(caught.exception.stage, "tooling")
        self.assertEqual(self.result_of("wrong-jar")["status"], "error")

    def test_tlc_timeout_keeps_partial_log(self):
        timeout = subprocess.TimeoutExpired(["java"], 60, output=b"partial TLC output",
                                            stderr=b"partial stderr")
        with patch("validate.subprocess.run", side_effect=timeout):
            with self.assertRaises(TraceError) as caught:
                self.check_records(self.records, "timeout")
        self.assertEqual(caught.exception.stage, "tooling")
        self.assertEqual(self.result_of("timeout")["status"], "error")
        log = (self.artifacts / "timeout" / "tlc.log").read_text()
        self.assertIn("partial TLC output", log)
        self.assertIn("partial stderr", log)


if __name__ == "__main__":
    unittest.main()
