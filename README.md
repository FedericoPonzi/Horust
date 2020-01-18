#Horust
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

### TODO:
* Improve handle RestartStrategy
* Add parser for Duration
* Connect stdout to somewhere (maybe by default to horust's stdout.);
* Wait for all processes to die, or until sigterm is received.
* Spawn all the processes with a new process group (to ease the shutdown via killpg)
* Echo simple service

### Features:
* Run it as a standalone program. 
    * horus -f myconfig.toml to run this service or horus -c mycommand, and be sure they keep running
* Run it with --web-server to enable a webserver 

### Related:
http://supervisord.org/installing.html


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
```
[service]
restart = "on-failure"
```

* `readiness` = `health|custom command`: If not present, the service will be considered ready as soon as has been spawned. Otherwise, use:
    * `health`: Use the same strategy defined in the health configuration, 
    * `custom command`: If the custom command is succesfull then your service is ready.
```toml
[service]
readiness = "health" 
```
* `restart-backoff` = `string`: If the service cannot be started, use this backoff time before retrying again.

### Healthness Check
You can check the healthness of your system using an endpoint.
 * You can use the enforce dependency to kill every dependent system.

```toml
[service]
name = "my-cool-service"
command = "curl google.com"
wd = "/tmp/"
restart = "always"
restart-backoff = "10s"
```