mod utils;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;
use std::time::Duration;
use utils::*;

fn handle_requests(listener: TcpListener, stop: Receiver<()>) -> std::io::Result<()> {
    listener.set_nonblocking(true).unwrap();
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let mut buffer = [0; 512];
                stream.read(&mut buffer)?;
                let response = b"HTTP/1.1 200 OK\r\n\r\n";
                stream.write_all(response).expect("Stream write");
            }
            Err(_error) => {
                if let Ok(()) | Err(TryRecvError::Disconnected) = stop.try_recv() {
                    break;
                } else {
                    thread::sleep(Duration::from_millis(150));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn test_http_healthcheck() -> Result<(), std::io::Error> {
    let (mut cmd, tempdir) = get_cli();
    let loopback = Ipv4Addr::new(127, 0, 0, 1);
    let socket = SocketAddrV4::new(loopback, 0);
    let listener = TcpListener::bind(socket)?;
    let port = listener.local_addr()?.port();
    let endpoint = format!("http://localhost:{}", port);
    let service = format!(
        r#"
[termination]
wait = "1s"
[restart]
strategy = "never"
[healthiness]
http-endpoint = "{}""#,
        endpoint
    );
    let script = r#"#!/usr/bin/env bash
    sleep 2
    "#;
    store_service_script(tempdir.path(), script, Some(service.as_str()), None);
    let (sender, receiver) = mpsc::sync_channel(0);
    let (stop_listener, sl_receiver) = mpsc::sync_channel(1);

    thread::spawn(move || {
        handle_requests(listener, sl_receiver).unwrap();
        sender.send(()).expect("Chan closed");
    });
    let mut cmd = cmd.args(vec!["--unsuccessful-exit-finished-failed"]);
    run_async(&mut cmd, true).recv_or_kill(Duration::from_secs(15));
    stop_listener.send(()).unwrap();
    receiver
        .recv_timeout(Duration::from_millis(3000))
        .expect("Failed to received response from handle_request");
    Ok(())
}
