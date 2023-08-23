use crate::proto::messages::horust_msg_message::MessageType::Request;
use crate::proto::messages::{
    horust_msg_message, horust_msg_request, horust_msg_response, HorustMsgError, HorustMsgMessage,
    HorustMsgRequest, HorustMsgResponse, HorustMsgServiceStatusResponse,
};
use crate::{HorustMsgServiceStatus, UdsConnectionHandler};
use anyhow::{anyhow, Result};
use log::{error, info};
use std::io::ErrorKind;
use std::os::unix::net::UnixListener;

pub trait CommandsHandlerTrait {
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
            .message_type
            .ok_or(anyhow!("No request found in message sent from client."))?;
        if let Request(HorustMsgRequest {
            request: Some(request),
        }) = received
        {
            match request {
                horust_msg_request::Request::StatusRequest(status_request) => {
                    info!("Requested status for {}", status_request.service_name);
                    let service_name = status_request.service_name.clone();

                    let service_status =
                        self.get_service_status(status_request.service_name.clone());
                    let response = service_status
                        .map(|status| {
                            new_horust_msg_service_status_response(
                                status_request.service_name,
                                status,
                            )
                        })
                        .unwrap_or_else(|| {
                            new_horust_msg_error_response(format!(
                                "Service {} not found.",
                                service_name
                            ))
                        });
                    uds_conn_handler.send_message(response)?;
                }
                horust_msg_request::Request::ChangeRequest(_) => {}
            };
        }
        Ok(())
    }

    fn get_service_status(&self, service_name: String) -> Option<HorustMsgServiceStatus>;
}

pub fn new_horust_msg_error_response(error: String) -> HorustMsgMessage {
    HorustMsgMessage {
        message_type: Some(horust_msg_message::MessageType::Response(
            HorustMsgResponse {
                response: Some(horust_msg_response::Response::Error(HorustMsgError {
                    error_string: error,
                })),
            },
        )),
    }
}

pub fn new_horust_msg_service_status_response(
    service_name: String,
    status: HorustMsgServiceStatus,
) -> HorustMsgMessage {
    HorustMsgMessage {
        message_type: Some(horust_msg_message::MessageType::Response(
            HorustMsgResponse {
                response: Some(horust_msg_response::Response::StatusResponse(
                    HorustMsgServiceStatusResponse {
                        service_name,
                        service_status: status.into(),
                    },
                )),
            },
        )),
    }
}
