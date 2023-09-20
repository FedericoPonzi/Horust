[<img src="https://github.com/FedericoPonzi/Horust/raw/master/res/horust-logo.png" width="300" align="center">](https://github.com/FedericoPonzi/Horust/raw/master/res/horust-logo.png)

[![CI](https://github.com/FedericoPonzi/horust/workflows/CI/badge.svg?branch=master&event=push)](https://github.com/FedericoPonzi/Horust/actions?query=workflow%3ACI) [![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE) [![Gitter chat](https://badges.gitter.im/gitterHQ/gitter.png)](https://gitter.im/horust-init/community)
 
[Horust](https://github.com/FedericoPonzi/Horust) is a supervisor / init system written in rust and designed to be run inside containers.

# Table of contents
* [Goals](#goals)
* [Status](#status)
* [Usage](#usage)
* [Contributing](#contributing)
* [License](#license)

## Goals
* **Supervision**: Be a fully-featured supervision system, designed to be run in containers (but not only).
* **Easy to Grasp**: Have code that is easy to understand, modify _and remove_ when the situation calls for it.
* **Completeness**: Be a drop-in replacement for your own `init` system.
* **Rock Solid**: Be your favorite Egyptian God, to trust across all use cases.

## Status
At this point, this should be considered Alpha software. As in, you can (and should) use it, but under your own discretion.
Horust can be used on macOS in development situations.  Due to limitations in the macOS API, subprocesses of supervised processes may not correctly be reaped when their parent process exits.

## Usage
Assume you'd like to create a website health monitoring system. You can create one using Horust and a small python script.
    
#### 1. Create a new directory: 
That will contain our services: 

```shell
mkdir -p /etc/horust/services
```

#### 2. Create your first Horust service:

> **Pro Tip:** You can also bootstrap the creation of a new service, by using `horust --sample-service > new_service.toml`.

Create a new configuration file for Horust under `/etc/horust/services/healthchecker.toml`:

```toml
command = "/tmp/healthcheck.py"
start-delay = "10s"
[restart]
strategy = "never"
``` 

A couple of notes are due here:
* This library uses [TOML](https://github.com/toml-lang/toml) for configuration, to go along nicely with Rust's chosen configuration language.
* There are many [_supported_](https://github.com/FedericoPonzi/Horust/blob/master/DOCUMENTATION.md) properties for your service file, but only `command` is _required_.

On startup, Horust will read this service file, and run the `command`. According to the restart strategy "`never`", as soon as the service has carried out its task it _will not restart_, and Horust will exit.

As you can see, it will run the `/tmp/healthcheck.py` Python script, which doesn't exist yet. Let's create it!

#### 3. Create your Python script:

Create a new Python script under `/tmp/healthcheck.py`:

```python
#!/usr/bin/env python3
import urllib.request
import sys
req = urllib.request.Request("https://www.google.com", method="HEAD")
resp = urllib.request.urlopen(req)
if resp.status == 200:
    sys.exit(0)
else:
    sys.exit(1)
```

Don't forget to make it executable:

```shell
chmod +x /tmp/healthcheck.py
```

#### 4. Build Horust:

This step is only required because we don't have a release yet, or if you like to live on the edge.

For building Horust, you will need [Rust](https://www.rust-lang.org/learn/get-started). As soon as it's installed, you can build Horust with Rust's `cargo`: 

```shell
cargo build --release
```

#### 4. Run Horust:

Now you can just:

```shell
./horust
```

By default Horust searches for services inside the `/etc/horust/services` folder (which we have created in step 1).

Every 10 seconds from now on, Horust will send an HTTP `HEAD` request to https://google.it. If the response is different than `200`, then there is an issue!

In this case, we're just exiting with a different exit code (i.e. a `1` instead of a `0`). But in real life, you could trigger other actions - maybe storing this information in a database for long-term analysis, or sending an e-mail to the website's owner.

#### 5. Finish up:

Use <kbd>Ctrl</kbd>+<kbd>C</kbd> to stop Horust. Horust will send a `SIGTERM` signal to all the running services, and if it doesn't hear back for a while - it will terminate them by sending an additional  `SIGKILL` signal.

---

Check out the [documentation](https://github.com/FedericoPonzi/Horust/blob/master/DOCUMENTATION.md) for a complete reference of the options available on the service config file. A general overview is available below as well:

```toml
command = "/bin/bash -c 'echo hello world'"
start-delay = "2s"
start-after = ["database", "backend.toml"]
stdout = "STDOUT"
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

[failure]
successful-exit-code = [ 0, 1, 255]
strategy = "ignore"

[termination]
signal = "TERM"
wait = "10s"
die-if-failed = ["db.toml"]

[environment]
keep-env = false
re-export = [ "PATH", "DB_PASS"]
additional = { key = "value"} 
```

## Contributing
Thanks for considering contributing to horust! To get started have a look on [CONTRIBUTING.md](https://github.com/FedericoPonzi/Horust/blob/master/CONTRIBUTING.md).

## License
Horust is provided under the MIT license. Please read the attached [license](https://github.com/FedericoPonzi/horust/blob/master/LICENSE) file.
