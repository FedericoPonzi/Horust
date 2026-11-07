use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use super::formats::{Event, HealthinessStatus, ShuttingDown};

struct TraceWriter<W: Write> {
    output: W,
    sequence: u64,
}

impl<W: Write> TraceWriter<W> {
    fn new(output: W) -> io::Result<Self> {
        let mut writer = Self {
            output,
            sequence: 0,
        };
        writer.record("header", json!({"format": "horust-formal"}))?;
        Ok(writer)
    }

    fn record(&mut self, kind: &str, data: Value) -> io::Result<()> {
        serde_json::to_writer(
            &mut self.output,
            &json!({"version": 4, "seq": self.sequence, "kind": kind, "data": data}),
        )?;
        self.output.write_all(b"\n")?;
        self.output.flush()?;
        self.sequence += 1;
        Ok(())
    }
}

struct Capture {
    writer: TraceWriter<File>,
    pending: BTreeMap<u64, String>,
    children: BTreeMap<i32, String>,
    early_reaps: BTreeSet<i32>,
    workers: BTreeSet<u64>,
    preparation_workers: BTreeSet<u64>,
}

impl Capture {
    fn write(&mut self, kind: &str, mut data: Value) {
        data["cause"] = json!(CAUSE.get());
        data["actor"] = json!(ACTOR.get());
        self.writer
            .record(kind, data)
            .expect("formal trace write failed");
    }
}

static CAPTURE: OnceLock<Option<Mutex<Capture>>> = OnceLock::new();
static IDS: AtomicU64 = AtomicU64::new(1);
static SERVICES: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
thread_local! {
    static CAUSE: Cell<u64> = const { Cell::new(0) };
    static ACTOR: Cell<u64> = const { Cell::new(0) };
    static OWNER_STEP: Cell<bool> = const { Cell::new(false) };
}

pub(crate) struct OwnerStep {
    nested: bool,
}

impl OwnerStep {
    pub(crate) fn enter() -> Self {
        Self {
            nested: OWNER_STEP.replace(true),
        }
    }

    pub(crate) fn is_outer(&self) -> bool {
        !self.nested
    }
}

impl Drop for OwnerStep {
    fn drop(&mut self) {
        OWNER_STEP.set(self.nested);
    }
}

fn capture() -> Option<&'static Mutex<Capture>> {
    CAPTURE
        .get_or_init(|| {
            std::env::var_os("HORUST_FORMAL_TRACE").map(|path| {
                let file = File::options()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .expect("cannot create HORUST_FORMAL_TRACE (must be a new file)");
                Mutex::new(Capture {
                    writer: TraceWriter::new(file).expect("cannot initialize formal trace"),
                    pending: BTreeMap::new(),
                    children: BTreeMap::new(),
                    early_reaps: BTreeSet::new(),
                    workers: BTreeSet::new(),
                    preparation_workers: BTreeSet::new(),
                })
            })
        })
        .as_ref()
}

pub(crate) fn enabled() -> bool {
    capture().is_some()
}

pub(crate) fn id() -> u64 {
    IDS.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn service(name: &str) -> String {
    let mut names = SERVICES
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .expect("formal service alias lock poisoned");
    let next = names.len();
    names
        .entry(name.into())
        .or_insert_with(|| format!("s{next}"))
        .clone()
}

pub(crate) fn set_cause(cause: u64) {
    CAUSE.set(cause);
}

pub(crate) fn cause() -> u64 {
    CAUSE.get()
}

pub(crate) fn set_actor(actor: u64) {
    ACTOR.set(actor);
}

pub(crate) fn actor() -> u64 {
    ACTOR.get()
}

pub(crate) fn record(kind: &str, mut data: Value) {
    if let Some(capture) = capture() {
        if let Some(name) = data["svc"].as_str() {
            data["svc"] = json!(service(name));
        }
        let mut capture = capture.lock().expect("formal trace lock poisoned");
        if kind == "worker_start" {
            capture
                .workers
                .insert(data["worker"].as_u64().expect("worker id"));
        } else if kind == "worker_end" {
            capture
                .workers
                .remove(&data["worker"].as_u64().expect("worker id"));
        } else if kind == "preparation_worker_end" {
            capture
                .preparation_workers
                .remove(&data["request"].as_u64().expect("request id"));
        }
        capture.write(kind, data);
    }
}

pub(crate) fn spawn_request(service: &str, receiver: u64) -> u64 {
    let service = self::service(service);
    let request = id();
    if let Some(capture) = capture() {
        let mut capture = capture.lock().expect("formal trace lock poisoned");
        capture.pending.insert(request, service.clone());
        capture.preparation_workers.insert(request);
        capture.write(
            "spawn_request",
            json!({"request": request, "svc": service, "receiver": receiver}),
        );
    }
    request
}

pub(crate) fn spawn_done(request: u64) {
    if let Some(capture) = capture() {
        let mut capture = capture.lock().expect("formal trace lock poisoned");
        capture.pending.remove(&request);
        capture.write("spawn_done", json!({"request": request}));
    }
}

pub(crate) fn birth(service: &str, pid: i32) {
    let service = self::service(service);
    if let Some(capture) = capture() {
        let mut capture = capture.lock().expect("formal trace lock poisoned");
        if capture.early_reaps.remove(&pid) {
            capture.write("ambiguous_birth", json!({"svc": service, "pid": pid}));
        }
        capture.children.insert(pid, service.clone());
        capture.write(
            "birth",
            json!({"svc": service, "pid": pid, "request": ACTOR.get()}),
        );
    }
}

pub(crate) fn reap(pid: i32, code: i32, mapped: Option<&str>) {
    let mapped = mapped.map(service);
    if let Some(capture) = capture() {
        let mut capture = capture.lock().expect("formal trace lock poisoned");
        if capture.children.remove(&pid).is_none() {
            capture.early_reaps.insert(pid);
        }
        capture.write("reap", json!({"pid": pid, "code": code, "svc": mapped}));
    }
}

pub(crate) fn complete() {
    if let Some(capture) = capture() {
        let mut capture = capture.lock().expect("formal trace lock poisoned");
        let data = json!({"pending": capture.pending, "unreaped": capture.children,
                          "workers": capture.workers,
                          "preparation_workers": capture.preparation_workers});
        capture.write("complete", data);
    }
}

pub(crate) fn event(event: &Event) -> Value {
    let mut value = json!({"kind": "", "svc": "", "target": "", "pid": 0,
                           "code": 0, "good": false, "mode": "", "attempt_id": 0});
    let (kind, service) = match event {
        Event::Run(s) => ("Run", s),
        Event::Kill(s) => ("Kill", s),
        Event::ForceKill(s) => ("ForceKill", s),
        Event::SpawnFailed(s) => ("SpawnFailed", s),
        Event::PidChanged(s, pid) => {
            value["pid"] = json!(pid.as_raw());
            ("PidChanged", s)
        }
        Event::StatusUpdate(s, status) | Event::StatusChanged(s, status) => {
            value["target"] = json!(status);
            (
                if matches!(event, Event::StatusUpdate(..)) {
                    "StatusUpdate"
                } else {
                    "StatusChanged"
                },
                s,
            )
        }
        Event::ServiceExited(s, code) => {
            value["code"] = json!(code);
            ("ServiceExited", s)
        }
        Event::StartHealthCheck(s, attempt_id) => {
            value["attempt_id"] = json!(attempt_id);
            ("StartHealthCheck", s)
        }
        Event::HealthCheck(s, attempt_id, health) => {
            value["attempt_id"] = json!(attempt_id);
            value["good"] = json!(*health == HealthinessStatus::Healthy);
            ("HealthCheck", s)
        }
        Event::ShuttingDownInitiated(mode) => {
            value["kind"] = json!("Shutdown");
            value["mode"] = json!(match mode {
                ShuttingDown::Gracefully => "Graceful",
                ShuttingDown::Forcefully => "Forceful",
            });
            return value;
        }
    };
    value["kind"] = json!(kind);
    value["svc"] = json!(self::service(service));
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trace_writer_orders_records_and_marks_completion() {
        let mut writer = TraceWriter::new(Vec::new()).unwrap();
        writer
            .record("initial", json!({"state": "Initial"}))
            .unwrap();
        writer.record("complete", json!({})).unwrap();
        let records: Vec<serde_json::Value> = String::from_utf8(writer.output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 3);
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record["seq"], index);
            assert_eq!(record["version"], 4);
        }
        assert_eq!(records[0]["kind"], "header");
        assert_eq!(records[2]["kind"], "complete");
    }

    #[test]
    fn trace_writer_surfaces_io_errors() {
        struct Broken;
        impl std::io::Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("deliberate trace failure"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(TraceWriter::new(Broken).is_err());
    }

    #[test]
    fn event_attempt_tags_are_explicit_and_preserve_the_source() {
        for event in [
            Event::Run("svc".into()),
            Event::StatusChanged("svc".into(), super::super::formats::ServiceStatus::Started),
            Event::ShuttingDownInitiated(ShuttingDown::Forcefully),
        ] {
            assert_eq!(super::event(&event)["attempt_id"], 0);
        }
        for attempt in [0, 17, u64::MAX] {
            let start = super::event(&Event::StartHealthCheck("svc".into(), attempt));
            let health = super::event(&Event::HealthCheck(
                "svc".into(),
                attempt,
                HealthinessStatus::Healthy,
            ));
            assert_eq!(start["kind"], "StartHealthCheck");
            assert_eq!(start["attempt_id"], attempt);
            assert_eq!(health["attempt_id"], attempt);
            assert_eq!(health["good"], true);
        }
    }

    #[test]
    fn owner_step_scope_suppresses_only_nested_steps_and_cleans_up_on_unwind() {
        let outer = OwnerStep::enter();
        assert!(outer.is_outer());
        {
            let nested = OwnerStep::enter();
            assert!(!nested.is_outer());
        }
        assert!(
            std::panic::catch_unwind(|| {
                let nested = OwnerStep::enter();
                assert!(!nested.is_outer());
                panic!("scope unwind");
            })
            .is_err()
        );
        assert!(!OwnerStep::enter().is_outer());
        drop(outer);
        assert!(OwnerStep::enter().is_outer());
    }
}
