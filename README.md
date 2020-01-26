# Horust
[![GHA Build Status](https://github.com/FedericoPonzi/horust/workflows/CI/badge.svg)](https://github.com/FedericoPonzi/horust/actions?query=workflow%3ACI)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

Horust is an supervisor system written in rust and designed to be run in containers. 

## How to use it
1. Define your services inside `/etc/horust/services/`.
An example service:
```toml
# mycoolapp.toml:
path = "/usr/bin/mycoolapp.sh"
restart = "always"
start-delay = "10s"
start-after = "my-other-service.toml"
``` 

### Related:
http://supervisord.org/installing.html
https://skarnet.org/software/s6/
https://github.com/OpenRC/openrc/blob/master/supervise-daemon-guide.md

### FAQ:
What happens to dependant process, if a dependency process dies?


## Configuration

### Service section
* **`name` = `string`**: Name of the service. If not defined, it will use the filename instead.
```toml
[service]
name = "my-cool-service" #Optional, will take filename.toml.
```
* `command` = `string`: Specify a command to run, or a full path. You can also add arguments. If a full path is not provided, the binary will be searched using the $PATH env variable.
```
[service]
command = "curl google.com"
```
```
[service]
command = "/home/federicoponzi/dev/main.sh"
```
* `wd` = `string`: Change the current working directory to the value of wd, before spawning the service.
```toml
[service]
wd = "/"
```

* `restart` = `always|on-failure|never`: Defines the restart strategy.
* `readiness` = `health|custom command`: If not present, the service will be considered ready as soon as has been spawned. Otherwise, use:
    * `health`: Use the same strategy defined in the health configuration, 
    * `custom command`: If the custom command is succesfull then your service is ready.
* `restart-backoff` = `string`: If the service cannot be started, use this backoff time before retrying again.

### Healthness Check
 * You can check the healthness of your system using an http endpoint.
 * You can use the enforce dependency to kill every dependent system.

```toml
[service]
name = "my-cool-service"
command = "curl google.com"
working_directory = "/tmp/"
restart = "always"
restart-backoff = "10s"
required = false
rediness = "/tmp/my-cool-service.ready"

[healthness]
http_endpoint = "http://localhost:2020/healthcheck"
# Future:
# tcp_endpoint = "localhost:2020"
# udp_endpoint = "localhost:2020"
# use a unix domain socket:
# http_endpoint = "/var/run/my_cool_service.uds"
```


## Horust configuration
Horust itself can be tuned and modified by using the following shiny parameters:
```bash
web_server = false
```