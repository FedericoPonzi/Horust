extern crate core;

mod proto;

use crate::proto::messages::horust_msg_message::RequestType;
pub use crate::proto::messages::HorustMsgServiceStatus;
use crate::proto::messages::{
    HorustMsgMessage, HorustMsgServiceStatusRequest, HorustMsgServiceStatusResponse,
};
use anyhow::{anyhow, Context, Result};
use log::{error, info};
use prost::Message;
use std::io::{ErrorKind, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

pub trait CommandsHandlerTrait {
    // "blocking" - execute in its own tokio task.
    fn start(&mut self) -> Result<()> {
        // put the server logic in a loop to accept several connections
        loop {
            self.accept().expect("TODO: panic message");
        }
    }
    fn get_unix_listener(&mut self) -> &mut UnixListener;
    fn accept(&mut self) -> Result<()> {
        match self.get_unix_listener().accept() {
            Ok((stream, _addr)) => {
                let conn_handler = UdsConnectionHandler::new(stream);
                if let Err(err) = self.handle_connection(conn_handler) {
                    error!("Error handling connection: {}", err);
                }
            }
            Err(e) => {
                let kind = e.kind();
                if !matches!(ErrorKind::WouldBlock, kind) {
                    error!("Error accepting connction: {e} - you might need to restart Horust.");
                }
            }
        };
        Ok(())
    }
    fn handle_connection(&self, mut uds_conn_handler: UdsConnectionHandler) -> Result<()> {
        let received = uds_conn_handler
            .receive_message()?
            .request_type
            .ok_or(anyhow!("No request found in message sent from client."))?;
        match received {
            RequestType::StatusRequest(status_request) => {
                info!("Requested status for {}", status_request.service_name);
                let service_status = self.get_service_status(status_request.service_name.clone());
                uds_conn_handler.send_message(new_horust_msg_service_status_response(
                    status_request.service_name,
                    service_status,
                ))?;
            }
            RequestType::StatusResponse(_) => {}
            RequestType::ChangeRequest(_) => {}
        };
        Ok(())
    }

    fn get_service_status(&self, service_name: String) -> HorustMsgServiceStatus;
}

pub fn new_horust_msg_service_status_response(
    service_name: String,
    status: HorustMsgServiceStatus,
) -> HorustMsgMessage {
    HorustMsgMessage {
        request_type: Some(RequestType::StatusResponse(
            HorustMsgServiceStatusResponse {
                service_name,
                service_status: status.into(),
            },
        )),
    }
}

pub struct ClientHandler {
    uds_connection_handler: UdsConnectionHandler,
}
impl ClientHandler {
    pub fn new_client(socket_path: &Path) -> Result<Self> {
        Ok(Self {
            uds_connection_handler: UdsConnectionHandler::new(
                UnixStream::connect(socket_path).context("Could not create stream")?,
            ),
        })
    }
    pub fn send_status_request(
        &mut self,
        service_name: String,
    ) -> Result<(String, HorustMsgServiceStatus)> {
        let status = HorustMsgMessage {
            request_type: Some(RequestType::StatusRequest(HorustMsgServiceStatusRequest {
                service_name,
            })),
        };
        self.uds_connection_handler.send_message(status)?;
        // server is waiting for EOF.
        self.uds_connection_handler
            .socket
            .shutdown(Shutdown::Write)?;
        //Reads all bytes until EOF in this source, appending them to buf.
        let received = self.uds_connection_handler.receive_message()?;
        info!("Client: received: {received:?}");
        match received
            .request_type
            .ok_or(anyhow!("Error receiving message"))?
        {
            RequestType::StatusResponse(resp) => Ok((
                resp.service_name,
                HorustMsgServiceStatus::from_i32(resp.service_status).unwrap(),
            )),
            _ => unreachable!(),
        }
    }

    pub fn client(mut self, service_name: String) -> Result<()> {
        let status = HorustMsgMessage {
            request_type: Some(RequestType::StatusRequest(HorustMsgServiceStatusRequest {
                service_name,
            })),
        };
        self.uds_connection_handler.send_message(status)?;
        // server is waiting for EOF.
        self.uds_connection_handler
            .socket
            .shutdown(Shutdown::Write)?;
        //Reads all bytes until EOF in this source, appending them to buf.
        let received = self.uds_connection_handler.receive_message()?;
        info!("Client: received: {received:?}");
        Ok(())
    }
}

/// socket_name should be the pid of the horust process.
pub fn get_path(socket_folder: &Path, socket_name: i32) -> PathBuf {
    socket_folder.join(format!("hourst-{socket_name}.sock"))
}

pub struct UdsConnectionHandler {
    socket: UnixStream,
}
impl UdsConnectionHandler {
    pub fn new(socket: UnixStream) -> Self {
        Self { socket }
    }
    pub fn send_message(&mut self, message: HorustMsgMessage) -> Result<()> {
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
        Ok(HorustMsgMessage::decode(buf.as_slice())?)
    }
}
