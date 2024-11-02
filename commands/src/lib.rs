mod proto;

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::info;

pub struct CommandsUdsConnectionHandler {
    socket: UnixStream,
}
impl CommandsUdsConnectionHandler {
    fn get_path(socket_folder: &Path, socket_name: u32) -> PathBuf {
        socket_folder.join(format!("hourst-{socket_name}.sock"))
    }
    fn new(socket: UnixStream) -> Self {
        Self { socket }
    }
    pub async fn new_client(socket_path: PathBuf) -> Result<Self> {
        Ok(Self {
            socket: UnixStream::connect(socket_path)
                .await
                .context("Could not create stream")?,
        })
    }

    pub async fn client(mut self) -> Result<()> {
        info!("client: sending data");
        self.socket
            .write_all(b"Hello?")
            .await // we write bytes, &[u8]
            .context("Failed at writing onto the unix stream")?;
        info!("client: Completed.");
        // server is waiting for EOF.
        self.socket.shutdown().await?;

        let mut buf = String::new();
        info!("client: reading back:");
        //Reads all bytes until EOF in this source, appending them to buf.
        self.socket
            .read_to_string(&mut buf)
            .await // we write bytes, &[u8]
            .context("Failed at writing onto the unix stream")?;
        info!("Client received: {}", buf);
        Ok(())
    }

    pub async fn server(mut self) -> Result<()> {
        let mut message = String::new();
        info!("Server: receving data");
        // Reads all bytes until EOF in this source, appending them to buf.
        self.socket
            .read_to_string(&mut message)
            .await
            .context("Failed at reading the unix stream")?;
        info!("Server: Received data: {message}");
        self.socket
            .write_all(message.as_bytes())
            .await
            .context("Failed at reading the unix stream")?;

        info!("Server: has written back {}", message);
        Ok(())
    }
}
pub struct CommandsUdsServer {
    unix_listener: UnixListener,
}
impl CommandsUdsServer {
    pub async fn new(socket_path: &Path) -> Result<Self> {
        Ok(Self {
            unix_listener: UnixListener::bind(socket_path)
                .context("Could not create the unix socket")?,
        })
    }
    pub async fn start(&mut self) -> Result<()> {
        // put the server logic in a loop to accept several connections
        loop {
            self.accept().await?;
        }
        Ok(())
    }
    pub async fn accept(&mut self) -> Result<()> {
        match self.unix_listener.accept().await {
            Ok((stream, _addr)) => {
                tokio::spawn(async move {
                    CommandsUdsConnectionHandler::new(stream)
                        .server()
                        .await
                        .unwrap();
                })
                .await?
            }
            Err(e) => {
                bail!("error accepting connction: {e}")
            }
        };
        Ok(())
    }
}

fn create_uds() {}

fn listen_uds() {}

fn send_message() {}
fn receive_message() {}
