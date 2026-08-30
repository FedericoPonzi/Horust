use crate::proto::messages::horust_msg_message::MessageType;
use crate::proto::messages::{
    HorustMsgAllServicesStatusRequest, HorustMsgMessage, HorustMsgRequest, HorustMsgRestartRequest,
    HorustMsgServiceChangeRequest, HorustMsgServiceStatusRequest, horust_msg_request,
    horust_msg_response,
};
use crate::{HorustMsgServiceStatus, UdsConnectionHandler};
use anyhow::{Context, anyhow};
use anyhow::{Result, bail};
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
            _ => Some(Ok(v)),
        };
    }
    None
}

fn send_and_receive(
    uds_connection_handler: &mut UdsConnectionHandler,
    message: HorustMsgMessage,
) -> Result<horust_msg_response::Response> {
    uds_connection_handler.send_message(message)?;
    uds_connection_handler.socket.shutdown(Shutdown::Write)?;
    let received = uds_connection_handler.receive_message()?;
    debug!("Client: received: {received:?}");
    unwrap_response(received).ok_or_else(|| anyhow!("No response received"))?
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
        let response = send_and_receive(&mut self.uds_connection_handler, status)?;
        if let horust_msg_response::Response::StatusResponse(resp) = response {
            Ok((
                resp.service_name,
                HorustMsgServiceStatus::try_from(resp.service_status).unwrap(),
            ))
        } else {
            bail!("Invalid response received: {:?}", response);
        }
    }

    pub fn send_change_request(
        &mut self,
        service_name: String,
        new_status: HorustMsgServiceStatus,
    ) -> Result<(String, bool)> {
        let msg = new_request(horust_msg_request::Request::ChangeRequest(
            HorustMsgServiceChangeRequest {
                service_name,
                service_status: new_status.into(),
            },
        ));
        let response = send_and_receive(&mut self.uds_connection_handler, msg)?;
        if let horust_msg_response::Response::ChangeResponse(resp) = response {
            Ok((resp.service_name, resp.accepted))
        } else {
            bail!("Invalid response received: {:?}", response);
        }
    }

    pub fn send_restart_request(&mut self, service_name: String) -> Result<(String, bool)> {
        let msg = new_request(horust_msg_request::Request::RestartRequest(
            HorustMsgRestartRequest { service_name },
        ));
        let response = send_and_receive(&mut self.uds_connection_handler, msg)?;
        if let horust_msg_response::Response::RestartResponse(resp) = response {
            Ok((resp.service_name, resp.accepted))
        } else {
            bail!("Invalid response received: {:?}", response);
        }
    }

    pub fn send_all_status_request(&mut self) -> Result<Vec<(String, HorustMsgServiceStatus)>> {
        let msg = new_request(horust_msg_request::Request::AllStatusRequest(
            HorustMsgAllServicesStatusRequest {},
        ));
        let response = send_and_receive(&mut self.uds_connection_handler, msg)?;
        if let horust_msg_response::Response::AllStatusResponse(resp) = response {
            let statuses = resp
                .services
                .into_iter()
                .filter_map(|entry| {
                    HorustMsgServiceStatus::try_from(entry.service_status)
                        .ok()
                        .map(|status| (entry.service_name, status))
                })
                .collect();
            Ok(statuses)
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
