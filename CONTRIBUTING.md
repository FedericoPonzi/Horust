# Contributing to Horust
Thanks for considering contributing to Horust! 

[Github Issue](https://github.com/FedericoPonzi/horust/issues) are a good place for getting started. 
You can also search the code for `TODO`s.

We should use [issues](https://github.com/FedericoPonzi/Horust/issues/new) to track things to do. Thus every PR should fix one or more issues. 
So even if you want to fix a TODO, please create an issue first. 
In this way it's easier to keep track of who's working on what.

## Development environment
In order to start hacking on Horust, you will need to install [Rust](https://www.rust-lang.org/tools/install) (1.42.0).
By using rustup, you will also automatically install cargo and other needed tools.

You can run horust with debug logs by using:
```bash
HORUST_LOG=debug cargo run 
```

Passing arguments to horust via cargo:
```bash
HORUST_LOG=debug cargo run -- --sample-service
```

Run Horust in single command mode:
```bash
HORUST_LOG=debug cargo run -- -- /bin/bash
```

## For PRs:
Before almost every commit, you might want to check your fmt:
```
cargo fmt
```

Clippy for lints:
```
cargo clippy
```

And run tests:
```
cargo test
```

If you want to run integration tests only:
```
cargo test --package horust --test horust -- --exact
```

There is also a make file, at the moment used mainly for docker:
```
# build a container without the http feature:
make build-nofeature
# run the built image
make run
# print an help:
make help 
```

---

## Local development using a container

Horust was developed to be used inside containers. That means that it's primarily built to be used in *nix environments,
and even more specifically in Linux and its flavours. At the time of writing, Horust fails to compile on Mac OSX due 
to a dependency (`prctl`) that does not exist on Mac, as well as some `libc` functions that are Linux-sepcific and are
not available on OSX. 

To solve this issue, the [`localdev`](https://github.com/FedericoPonzi/Horust/blob/edfbeaee6d56d2121a445ea6e249d471d62aac4f/localdev#L70-L70) folder contains a small set of files that aid in creating a container-based development workflow.

### Add some more info about process

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
* man runlevel
* man 8 init
* man getty
