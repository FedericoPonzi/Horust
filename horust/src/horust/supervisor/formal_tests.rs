use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::horust::formats::{FailureStrategy, HealthinessStatus, RestartStrategy};
use crate::horust::{bus::Bus, formal_trace as trace};
use rand::{RngExt, SeedableRng, rngs::StdRng};

pub(super) enum Effect {
    Spawn {
        service: Box<Service>,
        request: u64,
        origin: u64,
        delay: Duration,
        bus: BusConnector<Event>,
        sender: crossbeam_channel::Sender<process_spawner::Preparation>,
    },
    Fork {
        pid: i32,
    },
    Signal {
        pid: i32,
        signal: signal::Signal,
    },
}

struct Simulated {
    base: Instant,
    elapsed: Duration,
    effects: Vec<Effect>,
    next_pid: i32,
}

thread_local! {
    static SIMULATION: RefCell<Option<Simulated>> = const { RefCell::new(None) };
}

struct Simulation;
impl Simulation {
    fn new() -> Self {
        SIMULATION.with(|s| {
            assert!(s.borrow().is_none());
            *s.borrow_mut() = Some(Simulated {
                base: Instant::now(),
                elapsed: Duration::ZERO,
                effects: vec![],
                next_pid: 10000,
            });
        });
        Self
    }
}
impl Drop for Simulation {
    fn drop(&mut self) {
        SIMULATION.with(|s| *s.borrow_mut() = None);
    }
}

pub(super) fn now() -> Option<Instant> {
    SIMULATION.with(|s| s.borrow().as_ref().map(|s| s.base + s.elapsed))
}

pub(super) fn simulated() -> bool {
    now().is_some()
}

fn advance(duration: Duration) {
    SIMULATION.with(|s| s.borrow_mut().as_mut().expect("simulation active").elapsed += duration);
}

fn take_effects() -> Vec<Effect> {
    SIMULATION
        .with(|s| std::mem::take(&mut s.borrow_mut().as_mut().expect("simulation active").effects))
}

pub(super) fn intercept_spawn(
    service: Service,
    request: u64,
    origin: u64,
    delay: Duration,
    bus: BusConnector<Event>,
    sender: crossbeam_channel::Sender<process_spawner::Preparation>,
) {
    SIMULATION.with(|s| {
        let mut state = s.borrow_mut();
        let state = state.as_mut().expect("simulation active");
        state.effects.push(Effect::Spawn {
            service: Box::new(service),
            request,
            origin,
            delay,
            bus,
            sender,
        });
    })
}

pub(super) fn intercept_fork(service: &Service) -> unistd::Pid {
    SIMULATION.with(|s| {
        let mut state = s.borrow_mut();
        let state = state.as_mut().expect("simulation active");
        let pid = state.next_pid;
        state.next_pid += 1;
        trace::birth(&service.name, pid);
        state.effects.push(Effect::Fork { pid });
        unistd::Pid::from_raw(pid)
    })
}

pub(super) fn intercept_signal(sh: &ServiceHandler, signal: signal::Signal) -> bool {
    SIMULATION.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            if let Some(pid) = sh.pid {
                state.effects.push(Effect::Signal {
                    pid: pid.as_raw(),
                    signal,
                });
                trace::record(
                    "signal",
                    serde_json::json!({"svc": sh.name(), "pid": pid.as_raw(),
                    "signal": signal as i32, "errno": null, "simulated": true}),
                );
            }
            true
        } else {
            false
        }
    })
}

pub(super) fn order<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    if simulated() {
        values.sort();
    }
    values
}

struct Pending {
    service: Service,
    request: u64,
    origin: u64,
    ready: u64,
    bus: BusConnector<Event>,
    sender: crossbeam_channel::Sender<process_spawner::Preparation>,
}

struct Process {
    killed: bool,
    ignores_term: bool,
}

fn run_schedule(seed: u64, depth: u64) {
    let _simulation = Simulation::new();
    let mut rng = StdRng::seed_from_u64(seed);
    trace::record(
        "simulation",
        serde_json::json!({"seed": seed, "depth": depth}),
    );
    let count = 1 + (seed % 3) as usize;
    let mut services = Vec::new();
    for i in 0..count {
        let mut svc = Service::from_command("/must/not/be/executed".into());
        svc.name = format!("svc{i}");
        if i > 0 && seed % 2 == 1 {
            svc.start_after.push(format!("svc{}", i - 1));
        }
        if i + 1 < count && seed % 2 == 1 {
            svc.shutdown_after.push(format!("svc{}", i + 1));
        }
        svc.restart.strategy = match if seed == 0 { 0 } else { rng.random_range(0..3) } {
            0 => RestartStrategy::Never,
            1 => RestartStrategy::OnFailure,
            _ => RestartStrategy::Always,
        };
        svc.restart.attempts = if seed == 0 { 1 } else { rng.random_range(0..3) };
        svc.restart.backoff = Duration::from_secs(1);
        svc.start_delay = Duration::from_secs(seed % 3);
        svc.healthiness.max_failed = 1;
        svc.healthiness.command = Some("/bin/true".into());
        svc.healthiness.healthy_after = Duration::from_secs(seed % 2);
        svc.termination.wait = Duration::from_secs(2);
        svc.failure.strategy = match if seed == 0 { 0 } else { rng.random_range(0..3) } {
            0 => FailureStrategy::Ignore,
            1 => FailureStrategy::KillDependents,
            _ => FailureStrategy::Shutdown,
        };
        if i > 0 && rng.random_bool(0.3) {
            svc.termination.die_if_failed.push("svc0".into());
        }
        if seed % 5 == 1 {
            svc.failure.successful_exit_code.push(7);
        }
        services.push(svc);
    }
    let bus = Bus::new();
    let environment = bus.join_bus();
    let mut supervisor = Supervisor::new(bus.join_bus(), services);
    let mut pending = Vec::<Pending>::new();
    let mut processes = BTreeMap::<i32, Process>::new();
    let mut workers = BTreeMap::<String, Vec<(u64, u64, u64)>>::new();
    let mut worker_sources = BTreeMap::<u64, u64>::new();
    let mut finished = false;
    for tick in 0..depth {
        trace::record("schedule", serde_json::json!({"tick": tick}));
        bus.pump();
        if supervisor.repo.all_have_finished() {
            finished = true;
            break;
        }
        let mut produced = Vec::new();
        for (cause, ev) in supervisor.repo.bus.try_get_events_with_ids() {
            trace::set_cause(cause);
            produced.extend(supervisor.handle_event(ev));
        }
        trace::set_cause(0);
        let all_running = supervisor
            .repo
            .services
            .values()
            .all(ServiceHandler::is_running);
        let signals = if (seed == 0 && all_running && supervisor.status == LifecycleStatus::Running)
            || tick == depth / 2
            || tick == depth * 3 / 4
        {
            1
        } else {
            0
        };
        supervisor.handle_signals(signals);

        if seed == 0 || rng.random_bool(0.7) {
            for (cause, ev) in environment.try_get_events_with_ids() {
                trace::set_cause(cause);
                match ev {
                    Event::StartHealthCheck(service, attempt_id) => {
                        if let Some(old) = workers.remove(&service) {
                            for (worker, _, _) in old {
                                trace::record("worker_end", serde_json::json!({"worker": worker}));
                            }
                        }
                        let worker = trace::id();
                        worker_sources.insert(worker, cause);
                        trace::record(
                            "worker_start",
                            serde_json::json!({"worker": worker, "svc": service, "attempt_id": attempt_id}),
                        );
                        workers
                            .entry(service)
                            .or_default()
                            .push((worker, tick, attempt_id));
                    }
                    Event::ServiceExited(service, _) => {
                        if let Some(old) = workers.remove(&service) {
                            for (worker, _, _) in old {
                                trace::record("worker_end", serde_json::json!({"worker": worker}));
                            }
                        }
                    }
                    Event::ShuttingDownInitiated(_) => {
                        for old in workers.values() {
                            for (worker, _, _) in old {
                                trace::record("worker_end", serde_json::json!({"worker": worker}));
                            }
                        }
                        workers.clear();
                    }
                    _ => {}
                }
            }
        }
        trace::set_cause(0);
        for effect in take_effects() {
            match effect {
                Effect::Spawn {
                    service,
                    request,
                    origin,
                    delay,
                    bus,
                    sender,
                } => pending.push(Pending {
                    service: *service,
                    request,
                    origin,
                    ready: tick + delay.as_secs(),
                    bus,
                    sender,
                }),
                Effect::Signal { pid, signal } => {
                    if let Some(process) = processes.get_mut(&pid) {
                        process.killed = signal == signal::SIGKILL || !process.ignores_term;
                    }
                }
                Effect::Fork { .. } => panic!("fork before owner consumption"),
            }
        }
        let mut waiting = Vec::new();
        for spawn in pending {
            let cancelled = (seed == 0 || rng.random_bool(0.6))
                && spawn
                    .bus
                    .try_get_events_with_ids()
                    .into_iter()
                    .any(|(_, e)| matches!(e, Event::ShuttingDownInitiated(_)));
            if !cancelled && (tick < spawn.ready || (seed != 0 && rng.random_bool(0.2))) {
                waiting.push(spawn);
                continue;
            }
            trace::set_actor(spawn.request);
            trace::set_cause(spawn.origin);
            let result = if cancelled {
                Ok(None)
            } else if seed != 0 && rng.random_bool(0.15) {
                Err(anyhow::anyhow!("simulated preparation failure"))
            } else {
                process_spawner::prepare(spawn.service).map(Some)
            };
            trace::record(
                "preparation_result",
                serde_json::json!({
                    "request": spawn.request, "outcome": process_spawner::preparation_outcome(&result),
                }),
            );
            let _ = spawn.sender.send(result);
            trace::record(
                "preparation_worker_end",
                serde_json::json!({"request": spawn.request}),
            );
        }
        pending = waiting;
        trace::set_actor(0);
        trace::set_cause(0);
        produced.extend(supervisor.spawn_prepared());
        for effect in take_effects() {
            match effect {
                Effect::Fork { pid } => {
                    assert!(
                        supervisor
                            .repo
                            .get_service_by_pid(unistd::Pid::from_raw(pid))
                            .is_some(),
                        "virtual children must be registered before becoming reapable"
                    );
                    processes.insert(
                        pid,
                        Process {
                            killed: false,
                            ignores_term: seed != 0 && rng.random_bool(0.3),
                        },
                    );
                }
                _ => panic!("unexpected owner effect"),
            }
        }
        for (service, active) in &workers {
            for (worker, start, attempt_id) in active {
                trace::set_actor(*worker);
                trace::set_cause(worker_sources[worker]);
                let good = seed == 0 || rng.random_bool(0.8);
                let elapsed_ms = (tick - start) * 1000;
                if good
                    && u128::from(elapsed_ms)
                        < supervisor
                            .repo
                            .get_sh(service)
                            .service()
                            .healthiness
                            .healthy_after
                            .as_millis()
                {
                    continue;
                }
                trace::record(
                    "health_result",
                    serde_json::json!({"svc": service, "good": good, "elapsed_ms": elapsed_ms,
                        "attempt_id": attempt_id}),
                );
                Event::HealthCheck(
                    service.clone(),
                    *attempt_id,
                    if good {
                        HealthinessStatus::Healthy
                    } else {
                        HealthinessStatus::Unhealthy
                    },
                )
                .publish(&environment);
            }
        }
        trace::set_actor(0);
        trace::set_cause(0);
        let mut reaped = Vec::new();
        let exiting = processes
            .iter()
            .filter_map(|(pid, process)| {
                (process.killed || (seed != 0 && rng.random_bool(0.035))).then_some(*pid)
            })
            .collect::<Vec<_>>();
        for pid in exiting {
            let process = processes.remove(&pid).expect("known virtual process");
            let mapped = supervisor
                .repo
                .get_service_by_pid(unistd::Pid::from_raw(pid))
                .cloned();
            let code = if process.killed {
                -137
            } else if rng.random_bool(0.5) {
                0
            } else {
                7
            };
            trace::reap(pid, code, mapped.as_deref());
            if let Some(service) = mapped {
                reaped.push(Event::ServiceExited(service, code));
            }
        }
        for event in &reaped {
            produced.extend(supervisor.handle_reaped_event(event.clone()));
        }
        produced.extend(reaped);
        let next = supervisor.next_commands();
        supervisor.trace_generated(&next);
        supervisor.trace_publication(&produced, &next);
        for ev in produced.into_iter().chain(next) {
            ev.publish(&supervisor.repo.bus);
        }
        advance(Duration::from_secs(1));
    }
    trace::set_actor(0);
    trace::set_cause(0);
    if finished {
        supervisor.discard_preparations();
        trace::record(
            "supervisor_finished",
            serde_json::json!({"state": supervisor.trace_state()}),
        );
        supervisor.trace_publication(
            &[Event::ShuttingDownInitiated(ShuttingDown::Gracefully)],
            &[],
        );
        Event::ShuttingDownInitiated(ShuttingDown::Gracefully).publish(&supervisor.repo.bus);
        bus.pump();
        for spawn in pending {
            trace::set_actor(spawn.request);
            trace::set_cause(spawn.origin);
            trace::record(
                "preparation_result",
                serde_json::json!({"request": spawn.request, "outcome": "cancelled"}),
            );
            let _ = spawn.sender.send(Ok(None));
            trace::record(
                "preparation_worker_end",
                serde_json::json!({"request": spawn.request}),
            );
        }
        trace::set_actor(0);
        trace::set_cause(0);
        for active in workers.values() {
            for (worker, _, _) in active {
                trace::record("worker_end", serde_json::json!({"worker": worker}));
            }
        }
        bus.pump();
    } else {
        trace::record("watchdog", serde_json::json!({"depth": depth}));
    }
    drop(supervisor);
    drop(environment);
    drop(bus);
    if finished {
        trace::complete();
    }
}

#[test]
fn simulated_effects_do_not_spawn_or_signal_processes() {
    let _simulation = Simulation::new();
    let bus = crate::horust::bus::Bus::new();
    let service = Service::from_command("/must/not/be/executed".into());
    let receiver =
        process_spawner::prepare_service(service, Duration::ZERO, bus.join_bus(), (0, 0));
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    let mut sh = ServiceHandler::from(Service::default());
    sh.pid = Some(unistd::Pid::from_raw(i32::MAX));
    kill(&sh, Some(signal::SIGKILL));
    let effects = take_effects();
    assert_eq!(effects.len(), 2);
    assert!(matches!(effects[0], Effect::Spawn { .. }));
    assert!(matches!(effects[1], Effect::Signal { .. }));
}

#[test]
fn simulated_preparation_only_births_when_owner_registers() {
    let _simulation = Simulation::new();
    let bus = Bus::new();
    let mut service = Service::from_command("/must/not/be/executed".into());
    service.name = "service".into();
    let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
    supervisor.handle_event(Event::Run("service".into()));
    assert!(supervisor.repo.pid_map.is_empty());
    let Effect::Spawn {
        service, sender, ..
    } = take_effects().remove(0)
    else {
        panic!("expected preparation");
    };
    sender
        .send(Ok(Some(process_spawner::prepare(*service).unwrap())))
        .unwrap();
    assert!(supervisor.repo.pid_map.is_empty());
    let outputs = supervisor.spawn_prepared();
    let pid = supervisor.repo.get_sh("service").pid.unwrap();
    assert_eq!(
        outputs,
        vec![
            Event::PidChanged("service".into(), pid),
            Event::StatusChanged("service".into(), ServiceStatus::Started),
            Event::StartHealthCheck("service".into(), 1),
        ]
    );
    assert_eq!(
        supervisor.repo.get_service_by_pid(pid).map(String::as_str),
        Some("service")
    );
    supervisor.handle_event(Event::PidChanged(
        "service".into(),
        unistd::Pid::from_raw(99999),
    ));
    assert_eq!(supervisor.repo.get_sh("service").pid, Some(pid));
}

#[test]
fn owner_resolves_cancelled_failed_and_disconnected_preparations_once() {
    for outcome in ["cancelled", "failed", "disconnected"] {
        let _simulation = Simulation::new();
        let bus = Bus::new();
        let mut service = Service::from_command("/must/not/be/executed".into());
        service.name = "service".into();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
        supervisor.handle_event(Event::Run("service".into()));
        let Effect::Spawn { sender, .. } = take_effects().remove(0) else {
            panic!("expected preparation");
        };
        match outcome {
            "cancelled" => sender.send(Ok(None)).unwrap(),
            "failed" => sender
                .send(Err(anyhow::anyhow!("preparation failed")))
                .unwrap(),
            _ => {}
        }
        drop(sender);
        let context = (trace::actor(), trace::cause());
        let outputs = supervisor.spawn_prepared();
        assert_eq!(
            outputs,
            vec![Event::StatusUpdate("service".into(), ServiceStatus::Failed)]
        );
        assert_eq!(supervisor.repo.get_sh("service").restart_attempts, 1);
        assert!(supervisor.repo.pid_map.is_empty());
        assert!(take_effects().is_empty());
        assert!(supervisor.preparations.is_empty());
        assert!(supervisor.preparation_origins.is_empty());
        assert!(supervisor.spawn_prepared().is_empty());
        assert_eq!((trace::actor(), trace::cause()), context);
    }
}

#[test]
fn obsolete_ready_preparation_is_discarded_without_owner_fork() {
    let _simulation = Simulation::new();
    let bus = Bus::new();
    let mut service = Service::from_command("/must/not/be/executed".into());
    service.name = "service".into();
    let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
    supervisor.handle_event(Event::Run("service".into()));
    let Effect::Spawn {
        service, sender, ..
    } = take_effects().remove(0)
    else {
        panic!("expected preparation");
    };
    sender
        .send(process_spawner::prepare(*service).map(Some))
        .unwrap();
    supervisor.repo.get_mut_sh("service").status = ServiceStatus::Finished;
    let context = (trace::actor(), trace::cause());
    assert!(supervisor.spawn_prepared().is_empty());
    assert!(take_effects().is_empty());
    assert!(supervisor.repo.pid_map.is_empty());
    assert!(supervisor.preparation_origins.is_empty());
    assert_eq!((trace::actor(), trace::cause()), context);
}

#[test]
fn simulated_time_controls_production_deadline_observations() {
    let _simulation = Simulation::new();
    let before = clock_now();
    advance(Duration::from_secs(2));
    assert_eq!(clock_now().duration_since(before), Duration::from_secs(2));
}

#[test]
fn simulated_attempt_snapshots_advance_on_runs_and_ignore_stale_health() {
    let _simulation = Simulation::new();
    let bus = Bus::new();
    let mut service = Service::from_command("/must/not/be/executed".into());
    service.name = "service".into();
    let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
    let alias = trace::service("service");
    assert_eq!(
        supervisor.trace_state()["services"][&alias]["attempt_id"],
        0
    );
    for attempt in 1..=2 {
        supervisor.repo.get_mut_sh("service").status = ServiceStatus::Initial;
        supervisor.handle_event(Event::Run("service".into()));
        assert_eq!(
            supervisor.trace_state()["services"][&alias]["attempt_id"],
            attempt
        );
    }
    let before = supervisor.trace_state();
    assert!(
        supervisor
            .handle_event(Event::HealthCheck(
                "service".into(),
                1,
                HealthinessStatus::Unhealthy,
            ))
            .is_empty()
    );
    assert_eq!(supervisor.trace_state(), before);
}

#[test]
#[ignore = "requires an isolated HORUST_FORMAL_TRACE file"]
fn formal_exhaustion_trace() {
    let path = std::env::var_os("HORUST_FORMAL_TRACE").expect("trace path required");
    let read_steps = || {
        std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|record| record["kind"] == "step")
            .collect::<Vec<_>>()
    };
    for phase in [
        LifecycleStatus::Running,
        LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully),
        LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully),
    ] {
        let _simulation = Simulation::new();
        let bus = Bus::new();
        let mut service = Service::from_command("/must/not/be/executed".into());
        service.name = "service".into();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
        supervisor.status = phase;
        let handler = supervisor.repo.get_mut_sh("service");
        handler.attempt_id = u64::MAX;
        handler.restart_attempts = 2;
        handler.healthiness_checks_failed = Some(2);
        let before = supervisor.trace_state();
        let previous_steps = read_steps().len();
        Event::Run("service".into()).publish(&supervisor.repo.bus);
        bus.pump();
        let received = supervisor.repo.bus.try_get_events_with_ids();
        assert_eq!(received.len(), 1);
        let (source, event) = received.into_iter().next().unwrap();
        trace::set_cause(source);
        let input = trace::event(&event);
        let outputs = supervisor.handle_event(event);
        trace::set_cause(0);
        let steps = read_steps();
        assert_eq!(steps.len(), previous_steps + 1);
        let step = &steps.last().unwrap()["data"];
        assert_eq!(step["cause"], source);
        assert_eq!(step["actor"], 0);
        assert_eq!(step["input"], input);
        assert_eq!(step["before"], before);
        assert_eq!(step["after"], supervisor.trace_state());
        assert_eq!(
            step["outputs"],
            serde_json::json!(outputs.iter().map(trace::event).collect::<Vec<_>>())
        );
        let expected_phase = if phase == LifecycleStatus::ShuttingDown(ShuttingDown::Forcefully) {
            "Forceful"
        } else {
            "Graceful"
        };
        assert_eq!(step["after"]["phase"], expected_phase);
        let alias = trace::service("service");
        let after = &step["after"]["services"][&alias];
        assert_eq!(after["attempt_id"], u64::MAX);
        assert_eq!(after["retries"], 2);
        assert_eq!(after["hc"], 2);
        assert_eq!(after["status"], "FinishedFailed");
        assert!(take_effects().is_empty());
    }
    trace::complete();
}

fn assert_attempt_sources(records: &[serde_json::Value]) {
    fn assert_tags(value: &serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                if object.contains_key("kind") && object.contains_key("svc") {
                    assert!(value["attempt_id"].as_u64().is_some());
                    if !matches!(
                        value["kind"].as_str(),
                        Some("HealthCheck" | "StartHealthCheck")
                    ) {
                        assert_eq!(value["attempt_id"], 0);
                    }
                }
                if let Some(services) = object.get("services") {
                    for service in services.as_object().unwrap().values() {
                        assert!(service["attempt_id"].as_u64().is_some());
                    }
                }
                for child in object.values() {
                    assert_tags(child);
                }
            }
            serde_json::Value::Array(array) => array.iter().for_each(assert_tags),
            _ => {}
        }
    }
    let mut sent = BTreeMap::new();
    let mut workers = BTreeMap::new();
    for record in records {
        assert_eq!(record["version"], 4);
        assert_tags(record);
        let data = &record["data"];
        match record["kind"].as_str().unwrap() {
            "send" => {
                let event = &data["event"];
                if event["kind"] == "HealthCheck" {
                    let worker = workers
                        .get(&data["actor"].as_u64().unwrap())
                        .expect("health must retain its worker source");
                    let (service, attempt, source) = worker;
                    assert_eq!(&event["svc"], service);
                    assert_eq!(&event["attempt_id"], attempt);
                    assert_eq!(&data["cause"], source);
                }
                sent.insert(data["message"].as_u64().unwrap(), event.clone());
            }
            "worker_start" => {
                let start = &sent[&data["cause"].as_u64().unwrap()];
                assert_eq!(start["kind"], "StartHealthCheck");
                assert_eq!(start["svc"], data["svc"]);
                assert_eq!(start["attempt_id"], data["attempt_id"]);
                workers.insert(
                    data["worker"].as_u64().unwrap(),
                    (
                        data["svc"].clone(),
                        data["attempt_id"].clone(),
                        data["cause"].clone(),
                    ),
                );
            }
            "health_result" => {
                let (service, attempt, source) = &workers[&data["actor"].as_u64().unwrap()];
                assert_eq!(&data["svc"], service);
                assert_eq!(&data["attempt_id"], attempt);
                assert_eq!(&data["cause"], source);
            }
            "step" if data["input"]["kind"] == "HealthCheck" => {
                let input = &data["input"];
                assert_eq!(input, &sent[&data["cause"].as_u64().unwrap()]);
                let service = input["svc"].as_str().unwrap();
                if input["attempt_id"] != data["before"]["services"][service]["attempt_id"] {
                    assert_eq!(data["before"], data["after"]);
                    assert_eq!(data["outputs"], serde_json::json!([]));
                }
            }
            _ => {}
        }
    }
}

#[test]
#[ignore = "run one seeded schedule per subprocess through tools/formal"]
fn formal_schedule() {
    let seed = std::env::var("HORUST_FORMAL_SEED")
        .expect("seed required")
        .parse()
        .expect("u64 seed");
    let depth = std::env::var("HORUST_FORMAL_DEPTH")
        .unwrap_or_else(|_| "160".into())
        .parse()
        .expect("depth");
    run_schedule(seed, depth);
    if let Some(path) = std::env::var_os("HORUST_FORMAL_TRACE") {
        let records = std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect::<Vec<_>>();
        assert_attempt_sources(&records);
    }
}
