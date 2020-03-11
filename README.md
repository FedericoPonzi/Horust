# Horust [![GHA Build Status](https://github.com/FedericoPonzi/horust/workflows/CI/badge.svg)](https://github.com/FedericoPonzi/horust/actions?query=workflow%3ACI) [![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

Horust is an supervisor system written in rust and designed to be run in containers. 

# Table of contents
* Goals
* Status
* Usage
* Contributing
* License

## Goals:
* Supervision: A full fledge supervisor system, designed to be used in containers.
* Init system: Use Horust as your init system.
* Understandability: The code should be easy to understand and easy to modify.
* Rock solid: You should be able to trust your favorite egyptian God.

## Status
At this point, this should be considered Alpha software.

## Usage
1. Create a directory with your services. `/etc/horust/services/`.
An example service:
```toml
# mycoolapp.toml:
path = "/usr/bin/mycoolapp.sh"
restart-strategy = "always"
start-delay = "10s"
start-after = "my-other-service.toml"
``` 

Check the [documentation](https://github.com/FedericoPonzi/Horust/blob/master/DOCUMENTATION.md) for a complete reference.
You can also bootstrap the creation of a new service, by using `horust --sample-service > new_service.toml`.

```toml
name = "my-cool-service"
command = "/bin/bash -c 'echo hello world'"
working-directory = "/tmp/"
start-delay = "2s"
start-after = ["another.toml", "second.toml"]
user = "root"

[restart]
strategy = "never"
backoff = "0s"
attempts = 0

[healthiness]
http_endpoint = "http://localhost:8080/healthcheck"
file_path = "/var/myservice/up"

[failure]
exit_code = [ 1, 2, 3]
strategy = "ignore"

[termination]
signal = "TERM"
wait = "10s"
```

## Horust configuration
Horust itself can be tuned and modified by using the following shiny parameters:
```toml
# Default time to wait after sending a `sigterm` to a process before sending a SIGKILL.
timeout-before-sigkill = "10s"
```

## Contributing
Thanks for considering contributing to horust! 
[Github Issue](https://github.com/FedericoPonzi/horust/issues) are a good place for getting started. 
You can also search the code for `TODO`s.

If you're planning to add new features, it's super awesome but please open an [issue](https://github.com/FedericoPonzi/Horust/issues/new) describing your proposal before you start working on it.

Have a look on [DEVELOPMENT.md](https://github.com/FedericoPonzi/Horust/blob/master/DEVELOPMENT.md) for more info on how to get started hacking on horust.

## LICENSE
TBD
