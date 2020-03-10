## Development environment
In order to start hacking on Horust, you will need to install [Rust](https://www.rust-lang.org/tools/install) (1.41).
By using rustup, you will also automatically install cargo and other needed tools.

Run horust by using:
```bash
HORUST_LOG=debug cargo run 
```

Pass args to horust via cargo:
```bash
HORUST_LOG=debug cargo run -- --sample-service
```

Run a single command
```bash
HORUST_LOG=debug cargo run -- /bin/bash
```

### Useful Links:
Just a small collection of useful links:
* https://www.youtube.com/watch?v=gZqIEstv5lM
* https://docs.google.com/presentation/d/1jpAOBDiYfTvK3mWHuzrP8vaK7OUoALNCvegiibuvmYc/edit
* http://www.man7.org/linux/man-pages/man7/signal-safety.7.html
* https://github.com/tokio-rs/mio/issues/16 
* https://github.com/tokio-rs/mio/issues/16#issuecomment-102156238
* https://skarnet.org/software/s6/s6-svscan-1.html
* https://felipec.wordpress.com/2013/11/04/init/
* https://www.mustafaak.in/posts/2016-02-09-forking-process-in-myinit-go/
* https://blog.darknedgy.net/technology/2015/09/05/0/

### Random init systems:
* https://github.com/Yelp/dumb-init
* https://github.com/fpco/pid1
* https://github.com/krallin/tini/
* https://github.com/OpenRC/openrc
* https://github.com/OpenRC/openrc/blob/master/supervise-daemon-guide.md

### Useful man pages:
man runlevel
man 8 init
man getty
