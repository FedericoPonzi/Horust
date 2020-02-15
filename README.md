# Horust
[![GHA Build Status](https://github.com/FedericoPonzi/horust/workflows/CI/badge.svg)](https://github.com/FedericoPonzi/horust/actions?query=workflow%3ACI)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

Horust is an supervisor system written in rust and designed to be run in containers. 



# Table of contents
* Goals
* Usage
* Maintaier
* Contributing
* License



## Goals:
* Supervision: A full fledge supervisor system, designed to be used in containers.
* Init system: Use Horust as your init system.
* Understandability: The code should be easy to understand and easy to modify.
* Rock solid: You should be able to trust your favorite egyptian God.

## Status
At this point, this should be considered Alpha software. 
Check [Contributing](CONTRIBUTING.md) if you want to join the development.

## How to use it
1. Define your services inside `/etc/horust/services/`.
An example service:
```toml
# mycoolapp.toml:
path = "/usr/bin/mycoolapp.sh"
restart-strategy = "always"
start-delay = "10s"
start-after = "my-other-service.toml"
``` 

### Related:
http://supervisord.org/installing.html
https://skarnet.org/software/s6/
https://github.com/OpenRC/openrc/blob/master/supervise-daemon-guide.md

### FAQs:
What happens to dependant process, if a dependency process fails?


## Services
You can create new services by creating a toml file. Check the documentation below for a description of each field.
Bootstrap the creation of a new service, by using `horust --sample-service > new_service.toml`.
 
### Service section
* **`name` = `string`**: Name of the service. If not defined, it will use the filename instead.
* **`command` = `string`**: Specify a command to run, or a full path. You can also add arguments. If a full path is not provided, the binary will be searched using the $PATH env variable.
* **`working_directory` = `string`**: will use this value as current working directory for the service.

#### Restart section
* **`strategy` = `always|on-failure|never`**: Defines the restart strategy.
* **`backoff`** = `string`: If the service cannot be started, use this backoff time before retrying again.
* **`tentatives`** = `number`:
#### Rediness
* **`readiness` = `health`**: If not present, the service will be considered ready as soon as has been spawned. Otherwise, use:
    * **`health`**: Use the same strategy defined in the health configuration, 
    * **`custom command`**: If the custom command is successful then your service is ready.

### Healthness Check
 * You can check the healthness of your system using an http endpoint.
 * You can use the enforce dependency to kill every dependent system.

```toml
[service]
name = "my-cool-service"
command = "curl google.com"
working_directory = "/tmp/"
# If service cannot be started, bring the system down.
# Useful if you have some critical service you want to be sure it's running.
# default: false
required = false
# Rewrite incoming signals before proxying them:
signal_rewrite = "15:3,5:10"

[failure]
exit_code = "10,20"
# Shut down the system if this service fails.
strategy = "kill-all"

[restart]
strategy = "always"
backoff = "10s"
trials = 3
rediness = "/tmp/my-cool-service.ready"
[healthness]
http_endpoint = "http://localhost:2020/healthcheck"
file = "/var/myservice/up"
# Future:
# use a unix domain socket:
# http_endpoint = "/var/run/my_cool_service.uds"
# [environment]
# clear = true
# load = "/etc/my_db/env"
# Define directly in here:
# DATABASE_NAME = "My_DB"
# DATABASE_URI = "mysql@localhost"
# 
```


## Horust configuration
Horust itself can be tuned and modified by using the following shiny parameters:
```toml
# A web interface for managing horust.
web-server = false
# How much time to wait after sending a `sigterm` to a process before sending a SIGKILL.
timeout-before-sigkill = "10s"
```

## LICENSE
TBD

## Contributing
Thanks for considering contributing to horust! 
[Github Issue](https://github.com/FedericoPonzi/horust/issues) are a good place for getting started. 

If you're planning to add new features, it's super awesome but please let's discuss it via an issue before start working on it.

Have a look on [DEVELOPMENT.md](https://github.com/FedericoPonzi/Horust/blob/master/DEVELOPMENT.md) for more info on how to get started.
