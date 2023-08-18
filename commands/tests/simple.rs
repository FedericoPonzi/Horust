use anyhow::Result;
use horust_commands_lib::{CommandsUdsConnectionHandler, CommandsUdsServer};
use std::path::PathBuf;
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_simple() -> Result<()> {
    info!("Starting");

    let socket_path: PathBuf = "/tmp/simple.sock".into();
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }
    let socket_path2 = socket_path.clone();
    let s_handle = tokio::spawn(async move {
        let mut uds = CommandsUdsServer::new(&socket_path2).await.unwrap();
        info!("uds created");
        uds.accept().await.unwrap();
    });
    let c_handle = tokio::spawn(async {
        let client = CommandsUdsConnectionHandler::new_client(socket_path)
            .await
            .unwrap();
        client.client().await.unwrap();
    });
    s_handle.await?;
    c_handle.await?;
    Ok(())
}
