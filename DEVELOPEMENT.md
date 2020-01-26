## Development environment
In order to start hacking on Horust, you will need to install [Rust](https://www.rust-lang.org/tools/install) (1.40).
By using rustup, you will also automatically install cargo and other needed tools.

Run horust by using:
```bash
HORUST_LOG=debug cargo run 
```

Pass args to horust via cargo:
```bash
HORUST_LOG=debug cargo run -- --sample-service
```

### Useful Links:
Just a small collection of useful links:
* https://www.youtube.com/watch?v=gZqIEstv5lM
* https://docs.google.com/presentation/d/1jpAOBDiYfTvK3mWHuzrP8vaK7OUoALNCvegiibuvmYc/edit
* http://www.man7.org/linux/man-pages/man7/signal-safety.7.html
* https://github.com/tokio-rs/mio/issues/16 
* https://github.com/tokio-rs/mio/issues/16#issuecomment-102156238