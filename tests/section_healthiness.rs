mod utils;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use utils::*;

fn handle_request(listener: TcpListener) -> std::io::Result<()> {
    for stream in listener.incoming() {
        println!("Received request");
        let mut buffer = [0; 512];
        let mut stream = stream?;
        stream.read(&mut buffer).unwrap();
        let response = b"HTTP/1.1 200 OK\r\n\r\n";
        stream.write(response).expect("Stream write");
        break;
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
    while true ; do
        echo "sleeping.." 
        sleep 1
    done
    "#;
    store_service(tempdir.path(), script, Some(service.as_str()), None);
    let (sender, receiver) = mpsc::sync_channel(0);
    thread::spawn(move || {
        handle_request(listener).unwrap();
        sender.send(()).expect("Chan closed");
    });
    let mut cmd = cmd.args(vec!["--unsuccessful-exit-finished-failed"]);
    run_async(&mut cmd, false).recv_or_kill(Duration::from_secs(15));

    receiver
        .recv_timeout(Duration::from_millis(100))
        .expect("Failed to received response from handle_request");
    Ok(())
}
