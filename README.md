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

* **Supervision**: A fully-featured supervision system, designed to be run in containers (but not exclusively).
* **Simplicity**: Clear, modifiable, and removable code as needed.
* **Completeness**: A seamless drop-in for any `init` system.
* **Reliability**: Stability and trustworthiness across all use cases.

## Status

This should be considered Beta software. You can (and should) use it, but under your own
discretion. Please report any issue you encounter, or also sharing your use cases would be very helpful.
Horust can be used on macOS in development situations. Due to limitations in the macOS API, subprocesses of supervised
processes may not correctly be reaped when their parent process exits.

## Usage

Being a supervision and init system, it can be used to start and manage a bunch of processes. You can use it to
supervise a program and, for example, restart it in case it exists with an error. Or startup dependencies like start a
webserver after starting a database.

## Quick tutorial

As a simple example, assume you'd like to host your rest api. This is the code:

```
from http.server import BaseHTTPRequestHandler, HTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
         if self.path == "/user":
            raise Exception("Unsupported path: /user")  # Exception will kill the server
        self.send_response(200)
        self.send_header("Content-type", "text/plain")
        self.end_headers()
        self.wfile.write(b"Hello, World!")

HTTPServer(('', 8000), Handler).serve_forever()
```

you can run it using `python3 myapp.py`. If you go to localhost:8000/user, unfortunately, the server will fail. Now you
need to manually restart it!

Let's see how we can use horust to supervise it and restart it in case of failure.

#### 1. Create your first Horust service:

> [!TIP]
> You can also bootstrap the creation of a new service, by using `horust --sample-service > new_service.toml`.

We are now going to create a new config file for our service. They are defined
in [TOML](https://github.com/toml-lang/toml) and the default path where horust will look for service is in
`/etc/horust/services/`.

> [!NOTE]
> It's possible to run a one-shot instance just by doing `horust myprogram` without defining a service config file.

Let's create a new service under `/etc/horust/services/healthchecker.toml`:

```toml
command = "/tmp/myapp.py"
[restart]
strategy = "always"
``` 

There are many [_supported_](https://github.com/FedericoPonzi/Horust/blob/master/DOCUMENTATION.md) properties for your
service file, but only `command` is _required_.

On startup, Horust will read this service file, and run the `command` after waiting for 10 seconds. According to the
restart strategy "`never`", as
soon as the service has carried out its task it will restart.

As you can see, it will run the `/tmp/myapp.py` Python script, which doesn't exist yet. Let's create it!

#### 2. Create your app:

Create a new file script under `/tmp/myapp.py`:

```python
#!/usr/bin/env python3
from http.server import BaseHTTPRequestHandler, HTTPServer

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/user":
            raise Exception("Unsupported path: /user")  # Exception will kill the server
        self.send_response(200)
        self.send_header("Content-type", "text/plain")
        self.end_headers()
        self.wfile.write(b"Hello, World!")

HTTPServer(('', 8000), Handler).serve_forever()
```

And remember to make it executable:

```shell
chmod +x /tmp/api.py
```

#### 3. Get the latest release or build from source:

You can grab the latest release from the [releases](https://github.com/FedericoPonzi/Horust/releases/) page. Or if you
like to live on the edge, scroll down to the building section.

#### 4. Run Horust:

Now you can just:

```shell
./horust --uds-folder-path /tmp
```

> [!TIP]
> Horustctl is a program that allows you to interact with horust. They communicate using Unix Domain Socket (UDS),
> and by default horust stores the sockets in `/var/run/horust`.
> In this example, we have overridden the path by using the argument `--uds-folder-path`.

Try navigating to `http://localhost:8000/`. A page with Hello world
should be greeting you.

Now try navigating to `http://localhost:8000/user` - you should get a "the connection was reset" error page.
Checking on your terminal, you will see that the program has raised the exception, as we expected. Now, try navigating
again to `http://localhost:8000/` and the website is still up and running.

Pretty nice uh? One last thing!

If you downloaded a copy of horustctl, you can also do:

```
horustctl --uds-folder-path /tmp status myapp.toml
```

To check the status of your service. Currently, horustctl only support querying for the service status.

### 5. Wrapping up

Use <kbd>Ctrl</kbd>+<kbd>C</kbd> to stop Horust. Horust will send a `SIGTERM` signal to all the running services, and if
it doesn't hear back for a while - it will terminate them by sending an additional  `SIGKILL` signal. Wait time and
signals are configurable.

---

Check out the [documentation](https://github.com/FedericoPonzi/Horust/blob/master/DOCUMENTATION.md) for a complete
reference of the options available on the service config file. A general overview is available below as well:

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

[resource]
cpu-percent = 200
memory = "100 MiB"
```

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
