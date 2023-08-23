extern crate core;

mod client;
mod proto;
mod server;
use crate::proto::messages::HorustMsgMessage;
pub use crate::proto::messages::HorustMsgServiceStatus;
use anyhow::{Context, Result};
pub use client::ClientHandler;
use log::debug;
use prost::Message;
pub use server::CommandsHandlerTrait;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// socket_name should be the pid of the horust process.
pub fn get_path(socket_folder_path: &Path, horust_pid: i32) -> PathBuf {
    socket_folder_path.join(format!("hourst-{horust_pid}.sock"))
}

pub struct UdsConnectionHandler {
    socket: UnixStream,
}
impl UdsConnectionHandler {
    pub fn new(socket: UnixStream) -> Self {
        Self { socket }
    }
    pub fn send_message(&mut self, message: HorustMsgMessage) -> Result<()> {
        debug!("Sending message: {:?}", message);
        let mut buf = Vec::new();
        // Serialize the message into a byte array.
        message.encode(&mut buf)?;
        self.socket
            .write_all(&buf)
            .context("Failed at writing onto the unix stream")?;
        Ok(())
    }
    pub fn receive_message(&mut self) -> Result<HorustMsgMessage> {
        let mut buf = Vec::new();
        self.socket.read_to_end(&mut buf)?;
        let received = HorustMsgMessage::decode(buf.as_slice())?;
        debug!("Received message: {:?}", received);
        Ok(received)
    }
}
