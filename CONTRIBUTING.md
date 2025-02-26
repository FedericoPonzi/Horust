# Contributing to Horust

Thanks for considering contributing to Horust!

[GitHub Issues](https://github.com/FedericoPonzi/horust/issues) are a good place for getting started.
You can also search the code for `TODO`s.

We should use [issues](https://github.com/FedericoPonzi/Horust/issues/new) to track things to do. Thus, every PR should
fix one or more issues.
So even if you want to fix a TODO, please create an issue first.
In this way, it's easier to keep track of whom's working on what. If you need any help on clarifying the acceptance
criteria of an issue or work on the task, please do reach out.

## Development environment

To start hacking on Horust, you will need to install [Rust](https://www.rust-lang.org/tools/install) (>1.42.0) and
protoc, the protobuf compiler (apt-get install protobuf-compiler).
By using rustup, you will also automatically install cargo and other necessary tools.

You can run horust with debug logs on by using:

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

Cargo fix can also be used to fix common lint issues:

```
cargo fix
```

And run tests:

```
cargo test
```

If you want to run integration tests only:

```
cargo test --package horust --test horust -- --exact
```

There is also a make file, at the moment, used mainly for docker:

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
"All-In-One" Docker container. It's a wrapper around Horust that enables you to compile it and all of its dependencies,
and then work off the container instead of compiling locally.

The `Makefile` contains a set of commands intended to scaffold an AIO container from scratch.

Just run `make dargo COMMAND=X`, where `X` is any valid `cargo` command, inside the project's root folder.
If this is the first time you run the command, it will:

1. Create a Docker image with a pre-determined working directory
2. Run an (interactive / long-running) container off that image, with the local Horust project folder bind-mounted to
   the working directory
3. Run `cargo X` inside the container

When the Makefile target finishes, you will have a running container on your machine that you can compile Horust in.
That container allows you to take advantage of `rustc`'s incremental compilation, without compiling locally.

If it's not the first time you run the command, it will just run `cargo X` inside the container.

If you'd like to go for maximum ergonomics, run the following command (swapping `~/.bashrc` for `~/.zshrc` or wherever
you keep your shell stuff):

```bash
echo 'dargo(){ make dargo COMMAND=$1}' >> ~/.bashrc
source ~/.bashrc
```

This will now enable you to run `dargo X` instead of `make dargo COMMAND=X`, to get a more `cargo`-like feel while using
the container.
Try running `dargo check` to see how if feels!

### Container-based development example

Just to make sure everything is clear, let's run a step-by-step mini tutorial on developing in container-based
workflow "mode":

1. Clone the repo to your local machine:

```shell 
git clone https://github.com/FedericoPonzi/Horust.git`
cd Horust
```

2. Make some changes to a source file.
3. Run the following to create the AIO container and then test your changes:

```shell
make dargo COMMAND=test
```

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
