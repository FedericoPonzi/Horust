use crate::proto::messages::horust_msg_message::MessageType;
use crate::proto::messages::{
    horust_msg_request, horust_msg_response, HorustMsgMessage, HorustMsgRequest,
    HorustMsgServiceStatusRequest,
};
use crate::{HorustMsgServiceStatus, UdsConnectionHandler};
use anyhow::{anyhow, Context};
use anyhow::{bail, Result};
use log::{debug, info};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;

fn new_request(request_type: horust_msg_request::Request) -> HorustMsgMessage {
    HorustMsgMessage {
        message_type: Some(MessageType::Request(HorustMsgRequest {
            request: Some(request_type),
        })),
    }
}

// if anything is none it will return none
// if the response was an error it will return Some(Err).
fn unwrap_response(response: HorustMsgMessage) -> Option<Result<horust_msg_response::Response>> {
    if let MessageType::Response(resp) = response.message_type? {
        let v = resp.response?;
        return match &v {
            horust_msg_response::Response::Error(error) => {
                Some(Err(anyhow!("Error: {}", error.error_string)))
            }
            horust_msg_response::Response::StatusResponse(_status) => Some(Ok(v)),
        };
    }
    None
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
        let status = new_request(horust_msg_request::Request::StatusRequest(
            HorustMsgServiceStatusRequest { service_name },
        ));
        self.uds_connection_handler.send_message(status)?;
        // server is waiting for EOF.
        self.uds_connection_handler
            .socket
            .shutdown(Shutdown::Write)?;
        //Reads all bytes until EOF in this source, appending them to buf.
        let received = self.uds_connection_handler.receive_message()?;
        debug!("Client: received: {received:?}");
        let response = unwrap_response(received).unwrap()?;
        if let horust_msg_response::Response::StatusResponse(resp) = response {
            Ok((
                resp.service_name,
                HorustMsgServiceStatus::try_from(resp.service_status).unwrap(),
            ))
        } else {
            bail!("Invalid response received: {:?}", response);
        }
    }

    pub fn client(mut self, service_name: String) -> Result<()> {
        let received = self.send_status_request(service_name)?;
        info!("Client: received: {received:?}");
        Ok(())
    }
}
