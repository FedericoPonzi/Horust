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

If you'd like to skip all the bells and whistles of setting up the project, you can get up and running quickly using an
"All-In-One" Docker solution. It's basically a wrapper around Horust that enables you to compile it and all of its dependencies,
and then work off the binary instead of compiling yourself.

One example would be if you're developing on a system that does not support a certain Linux command that Horust uses.
Mac users, for example, would prefer this approach (since `prctl` is not available on Mac OSX).

To solve this issue, the `Makefile` contains a `dlocal CARGO_COMMAND` command, where `CARGO_COMMAND` is passed to `cargo`
inside the container. This flow allows for a container-based development workflow that's similar to the normal one.
Some examples:

`dlocal build` - Compiles Horust and its dependecies (need to run this once, to compile all the dependencies)
`dlocal test`  - Runs the test suite without re-compiling all dependencies
`dlocal run`   - Runs Horust inside a container
`dlocal check` - Performs 

### Container-based development example

ADD EXAMPLE

## Useful Links:
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

## Random init systems:
* https://github.com/Yelp/dumb-init
* https://github.com/fpco/pid1
* https://github.com/krallin/tini/
* https://github.com/OpenRC/openrc
* https://github.com/OpenRC/openrc/blob/master/supervise-daemon-guide.md

## Useful man pages:
* man runlevel
* man 8 init
* man getty
