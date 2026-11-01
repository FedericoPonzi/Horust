use super::*;
use crate::horust::bus::Bus;
use std::process::{Command, Stdio};
use std::time::Instant;

#[test]
fn pid_notifications_do_not_change_service_state() {
    let bus = Bus::new();
    let mut supervisor = Supervisor::new(bus.join_bus(), vec![Service::from_name("service")]);
    let pid = unistd::Pid::from_raw(12345);
    assert!(
        supervisor
            .handle_event(Event::PidChanged("service".into(), pid))
            .is_empty()
    );
    assert_eq!(
        supervisor.repo.get_sh("service").status,
        ServiceStatus::Initial
    );
    assert!(supervisor.repo.pid_map.is_empty());
}

#[test]
fn prepared_children_are_registered_before_reaping() {
    const CHILD_ENV: &str = "HORUST_REGISTRATION_TEST_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        prepared_registration();
        return;
    }

    // waitpid(-1) must not consume children belonging to other unit tests.
    let logs = tempfile::Builder::new()
        .prefix("horust-registration-")
        .tempdir()
        .unwrap()
        .keep();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "horust::supervisor::registration_tests::prepared_children_are_registered_before_reaping",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .stdout(Stdio::from(
            std::fs::File::create(logs.join("stdout.log")).unwrap(),
        ))
        .stderr(Stdio::from(
            std::fs::File::create(logs.join("stderr.log")).unwrap(),
        ))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "registration reproduction failed; logs: {logs:?}"
            );
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("registration reproduction timed out; logs: {logs:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn prepared_registration() {
    let bus = Bus::<Event>::new();
    let mut services = Vec::new();
    let cases = [
        (
            "successful",
            "/bin/sh -c 'exit 0'",
            0,
            ServiceStatus::Success,
        ),
        ("failed", "/bin/sh -c 'exit 7'", 7, ServiceStatus::Failed),
        (
            "signaled",
            "/bin/sh -c 'kill -TERM $$'",
            -137,
            ServiceStatus::Failed,
        ),
    ];
    for (name, command, _, _) in &cases {
        let mut service = Service::from_command((*command).into());
        service.name = (*name).into();
        services.push(service);
    }
    for (name, command, delay) in [
        ("missing", "horust-registration-no-such-command", 0),
        ("cancelled", "/bin/true", 60),
    ] {
        let mut service = Service::from_command(command.into());
        service.name = name.into();
        service.start_delay = Duration::from_secs(delay);
        services.push(service);
    }
    let mut supervisor = Supervisor::new(bus.join_bus(), services);
    let bus_thread = thread::spawn(move || bus.run());

    for (name, _, code, status) in cases {
        supervisor.handle_event(Event::Run(name.into()));
        let prepared = supervisor.preparations[name]
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            nix::sys::wait::waitpid(
                unistd::Pid::from_raw(-1),
                Some(nix::sys::wait::WaitPidFlag::WNOHANG)
            ),
            Err(nix::errno::Errno::ECHILD),
            "preparation must not fork"
        );
        reap_unrelated_child(&supervisor);

        queue_preparation(&mut supervisor, name, prepared);
        let notifications = supervisor.spawn_prepared();
        let pid = supervisor.repo.get_sh(name).pid.unwrap();
        assert_eq!(
            supervisor.repo.get_service_by_pid(pid).map(String::as_str),
            Some(name)
        );
        assert_eq!(
            notifications,
            vec![
                Event::PidChanged(name.into(), pid),
                Event::StatusChanged(name.into(), ServiceStatus::Started),
                Event::StartHealthCheck(name.into(), supervisor.repo.services[name].attempt_id),
            ]
        );
        supervisor.handle_event(notifications[0].clone());
        assert_eq!(supervisor.repo.get_sh(name).pid, Some(pid));
        wait_until_exited(pid);
        let exits = reaper::run(&supervisor.repo, MAX_PROCESS_REAPS_ITERS);
        assert!(
            matches!(exits.as_slice(), [Event::ServiceExited(service, exit_code)]
                if service == name && *exit_code == code),
            "the exit was lost before registration: {exits:?}"
        );
        for exit in exits {
            supervisor.handle_event(exit);
        }
        assert_eq!(supervisor.repo.get_sh(name).status, status);
        assert_eq!(supervisor.repo.get_sh(name).pid, None);
        assert!(supervisor.repo.pid_map.is_empty());
        assert!(reaper::run(&supervisor.repo, MAX_PROCESS_REAPS_ITERS).is_empty());
        supervisor.handle_event(Event::PidChanged(name.into(), pid));
        assert_eq!(supervisor.repo.get_sh(name).status, status);
        assert!(supervisor.repo.pid_map.is_empty());
    }

    for name in ["missing", "cancelled"] {
        supervisor.handle_event(Event::Run(name.into()));
        if name == "cancelled" {
            supervisor
                .repo
                .send_ev(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
        }
        let prepared = supervisor.preparations[name]
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(prepared.as_ref().ok().and_then(Option::as_ref).is_none());
        queue_preparation(&mut supervisor, name, prepared);
        supervisor.spawn_prepared();
        assert_eq!(supervisor.repo.get_sh(name).status, ServiceStatus::Failed);
        assert_eq!(supervisor.repo.get_sh(name).restart_attempts, 1);
    }

    reap_unrelated_child(&supervisor);
    drop(supervisor);
    bus_thread.join().unwrap();
}

fn reap_unrelated_child(supervisor: &Supervisor) {
    let mut unrelated = Command::new("/bin/true").spawn().unwrap();
    let pid = unistd::Pid::from_raw(unrelated.id() as i32);
    wait_until_exited(pid);
    assert!(reaper::run(&supervisor.repo, MAX_PROCESS_REAPS_ITERS).is_empty());
    assert_eq!(
        nix::sys::wait::waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)),
        Err(nix::errno::Errno::ECHILD),
        "preparation must not prevent reaping unrelated children"
    );
    assert!(unrelated.wait().is_err());
}

fn queue_preparation(
    supervisor: &mut Supervisor,
    name: &str,
    result: process_spawner::Preparation,
) {
    let (sender, receiver) = crossbeam::channel::bounded(1);
    sender.send(result).unwrap();
    supervisor.preparations.insert(name.into(), receiver);
}

#[test]
fn cancelled_preparations_resolve_without_forking() {
    for (phase, status, retries) in [
        (
            LifecycleStatus::ShuttingDown(ShuttingDown::Gracefully),
            ServiceStatus::Starting,
            1,
        ),
        (LifecycleStatus::Running, ServiceStatus::InKilling, 0),
    ] {
        let bus = Bus::new();
        let mut service = Service::from_command("/bin/true".into());
        service.name = "service".into();
        let prepared = process_spawner::prepare(service.clone()).unwrap();
        let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
        supervisor.status = phase;
        supervisor.repo.get_mut_sh("service").status = status;
        queue_preparation(&mut supervisor, "service", Ok(Some(prepared)));
        supervisor.spawn_prepared();
        assert_eq!(
            supervisor.repo.get_sh("service").status,
            ServiceStatus::Failed
        );
        assert_eq!(supervisor.repo.get_sh("service").restart_attempts, retries);
        assert!(supervisor.repo.pid_map.is_empty());
        assert!(supervisor.spawn_prepared().is_empty());
        assert_eq!(supervisor.repo.get_sh("service").restart_attempts, retries);
        supervisor.handle_event(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
        let next = supervisor.repo.services["service"].next(&supervisor.repo, supervisor.status);
        for event in next {
            supervisor.handle_event(event);
        }
        assert!(supervisor.repo.all_have_finished());
    }
}

#[test]
fn old_preparations_cannot_complete_a_new_attempt() {
    let bus = Bus::new();
    let mut service = Service::from_command("/bin/true".into());
    service.name = "service".into();
    service.start_delay = Duration::from_secs(60);
    let prepared = process_spawner::prepare(service.clone()).unwrap();
    let mut supervisor = Supervisor::new(bus.join_bus(), vec![service]);
    let bus_thread = thread::spawn(move || bus.run());
    let (old_sender, old_receiver) = crossbeam::channel::bounded(1);
    supervisor
        .preparations
        .insert("service".into(), old_receiver);
    old_sender.send(Ok(Some(prepared))).unwrap();
    supervisor.handle_event(Event::Run("service".into()));
    assert!(
        old_sender
            .send(Err(anyhow::anyhow!("old failure")))
            .is_err()
    );
    assert!(supervisor.spawn_prepared().is_empty());
    assert_eq!(
        supervisor.repo.get_sh("service").status,
        ServiceStatus::Starting
    );
    assert_eq!(supervisor.repo.get_sh("service").restart_attempts, 0);
    supervisor
        .repo
        .send_ev(Event::ShuttingDownInitiated(ShuttingDown::Gracefully));
    let cancelled = supervisor.preparations["service"]
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(matches!(cancelled, Ok(None)));
    queue_preparation(&mut supervisor, "service", cancelled);
    supervisor.spawn_prepared();
    assert_eq!(
        supervisor.repo.get_sh("service").status,
        ServiceStatus::Failed
    );
    assert_eq!(supervisor.repo.get_sh("service").restart_attempts, 1);
    drop(supervisor);
    bus_thread.join().unwrap();
}

#[test]
fn disconnected_preparation_fails_once_but_terminal_results_do_not() {
    let bus = Bus::new();
    let mut supervisor = Supervisor::new(bus.join_bus(), vec![Service::from_name("service")]);
    supervisor.repo.get_mut_sh("service").status = ServiceStatus::Starting;
    let (sender, receiver) = crossbeam::channel::bounded(1);
    supervisor.preparations.insert("service".into(), receiver);
    drop(sender);
    supervisor.spawn_prepared();
    assert_eq!(
        supervisor.repo.get_sh("service").status,
        ServiceStatus::Failed
    );
    assert_eq!(supervisor.repo.get_sh("service").restart_attempts, 1);
    supervisor.repo.get_mut_sh("service").status = ServiceStatus::FinishedFailed;
    queue_preparation(
        &mut supervisor,
        "service",
        Err(anyhow::anyhow!("late failure")),
    );
    assert!(supervisor.spawn_prepared().is_empty());
    assert_eq!(
        supervisor.repo.get_sh("service").status,
        ServiceStatus::FinishedFailed
    );
    assert_eq!(supervisor.repo.get_sh("service").restart_attempts, 1);
}

fn wait_until_exited(pid: unistd::Pid) {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // Observe the exact child's exit without consuming its wait status.
    assert_eq!(
        unsafe {
            libc::waitid(
                libc::P_PID,
                pid.as_raw() as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOWAIT,
            )
        },
        0,
        "could not observe child {pid}: {}",
        std::io::Error::last_os_error()
    );
}
