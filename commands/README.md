## Compile
To compile this crate, you will need protobuf compiler. On debian-like you can run:

apt-get install protobuf-compiler


## TODOs:
* do I need to https://docs.rs/tokio/latest/tokio/net/struct.UnixStream.html 
 `let ready = stream.ready(Interest::READABLE | Interest::WRITABLE).await?;`


https://docs.rs/tokio/latest/tokio/net/struct.UnixListener.html#method.accept
