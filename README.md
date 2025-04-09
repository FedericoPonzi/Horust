[<img src="https://github.com/FedericoPonzi/Horust/raw/master/res/horust-logo.png" width="300" align="center">](https://github.com/FedericoPonzi/Horust/raw/master/res/horust-logo.png)

[![CI](https://github.com/FedericoPonzi/horust/workflows/CI/badge.svg?branch=master&event=push)](https://github.com/FedericoPonzi/Horust/actions?query=workflow%3ACI) [![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

[Horust](https://github.com/FedericoPonzi/Horust) is a supervisor / init system written in rust and designed to be run
inside containers.

# Table of contents

* [Goals](#goals)
* [Status](#status)
* [Usage](#usage)
* [Contributing](#contributing)
* [License](#license)

## Goals

* **Supervision**: A fully-featured supervisor system, easy to use and designed to be run in containers (but not
  exclusively).
* **Simplicity**: Simple to use and simple to modify.
* **Completeness**: A seamless drop-in for any `init` or supervisor system.
* **Reliability**: Written using safe and correct wrappers.

## Status

This should be considered Beta software. You can (and should) use it, but under your own
discretion. Please report any issue you encounter, or also sharing your use cases would be very helpful.
Horust can be used on macOS in development situations. Due to limitations in the macOS API, subprocesses of supervised
processes may not correctly be reaped when their parent process exits.

## Usage

Being a supervision and init system, it can be used to start and manage a bunch of processes. You can use it to
supervise a program and, for example, restart it in case it exists with an error. Or startup dependencies like start a
webserver after starting a database.

Keep scrolling for a quick tutorial or check out the [documentation](https://gh.fponzi.me/Horust) for a complete
reference of the options available on the
service config file.

This is a quick overview:

```toml
command = "/bin/bash -c 'echo hello world'"
start-delay = "2s"
start-after = ["database", "backend.toml"]
stdout = "STDOUT"
stdout-rotate-size = "100MB"
stdout-should-append-timestamp-to-filename = false
stderr = "/var/logs/hello_world_svc/stderr.log"
user = "root"
working-directory = "/tmp/"

[restart]
strategy = "never"
backoff = "0s"
attempts = 0

[healthiness]
http-endpoint = "http://localhost:8080/healthcheck"
file-path = "/var/myservice/up"
command = "curl -s localhost:8080/healthcheck"

[failure]
successful-exit-code = [0, 1, 255]
strategy = "ignore"

[termination]
signal = "TERM"
wait = "10s"
die-if-failed = ["db.toml"]

[environment]
keep-env = false
re-export = ["PATH", "DB_PASS"]
additional = { key = "value" }

[resource-limit]
cpu = 0.5
memory = "100 MiB"
pids-max = 100
```

## How to get started with Horust:

You can grab the latest release from the [releases](https://github.com/FedericoPonzi/Horust/releases/) page. If you
like to live on the edge, scroll down to the building section.

There are docker releases:

* Github: https://github.com/FedericoPonzi/Horust/pkgs/container/horust
* Dockerhub: https://hub.docker.com/r/federicoponzi/horust

You can also pull horust as a library from [crates.io](https://crates.io/crates/horust), or use cargo to
install it:

```
cargo install horust
```

## Horustctl:

Horustctl is a program that allows you to interact with horust. They communicate using Unix Domain Socket (UDS), and by
default, horust stores the sockets in /var/run/horust. You can override the path by using the argument
--uds-folder-path. Then you can use it like this:

```
horustctl --uds-folder-path /tmp status myapp.toml
```

To check the status of your service. Currently, horustctl only supports querying for the service status.

## Quick tutorial

### Problem:

Assume you have a container in which you want to run a database and a monitoring system. The database should be started
first, and the monitoring system should be started only after the database is up.

A container can spin up a single process, so you create a simple bash file to handle the dependency.
Then, if the monitoring system fails or the database fails, you want to restart it. While you figure the different
requirements, your simple bash script keeps growing.

Let's see how we can use horust to spin up the two services and monitor them.

### 1. Create your first Horust service:

> [!TIP]
> You can also bootstrap the creation of a new service, by using `horust --sample-service > new_service.toml`.

Each program we need to spin up has its own service config file. They are defined
in [TOML](https://github.com/toml-lang/toml) and the default path where horust will look for service is in
`/etc/horust/services/`.

> [!NOTE]
> It's possible to run a one-shot instance just by doing `horust myprogram` without defining a service config file.

Let's create a new service under `/etc/horust/services/database.toml`:

```toml
command = "python3 /opt/db/my-cool-database.py --bind 0.0.0.0 --port 5323"
start-delay = "2s"
[restart]
strategy = "always"
``` 

and another service for the monitoring:  `/etc/horust/services/monitoring.toml`:

```toml
command = "python3 /opt/db/monitoring.py --port 5323"
start-after = ["database.toml"]
working-directory = "/tmp/"
```

There are many [_supported_](https://gh.fponzi.me/Horust) properties for your
service file, but only `command` is _required_.

On startup, Horust will read this service file. According to the restart strategy "`never`", as
soon as the service has carried out its task it will restart.

As you can see, it will run the `/tmp/myapp.py` Python script, which doesn't exist yet. Let's create it!

### 2. Define your container:

```dockerfile
FROM federicoponzi/horust:v0.2.0
# Install dependencies for my cool db
RUN apt-get update && \
    apt-get install -y python3 python3-pip && \
    pip3 install requests

COPY db /opt/db/

# Copy Horust service definition into the container
COPY database.toml /etc/horust/services/
COPY monitoring.toml /etc/horust/services/

# Set entrypoint to Horust
ENTRYPOINT ["/sbin/horust"]
```

and we're ready to start our container: `docker build -t my-cool-db .` and
`docker run -it --rm --name my-cool-db my-cool-db`.

### 3. Terminate the container

Use <kbd>Ctrl</kbd>+<kbd>C</kbd> to stop Horust. Horust will send a `SIGTERM` signal to all the running services, and if
it doesn't hear back for a while - it will terminate them by sending an additional  `SIGKILL` signal. Wait time and
signals are configurable.

## Building

For building Horust, you will need [Rust](https://www.rust-lang.org/learn/get-started) and `protoc` compiler. Protoc is
used for interacting with horust through horustctl.
As soon as both are installed, you can build Horust with:

```shell
cargo build --release
```

## Contributing

Thanks for considering contributing to horust! To get started, have a look
at [CONTRIBUTING.md](https://github.com/FedericoPonzi/Horust/blob/master/CONTRIBUTING.md).

## License

Horust is provided under the MIT license. Please read the
attached [license](https://github.com/FedericoPonzi/horust/blob/master/LICENSE) file.
