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
* `path` = `string`: Path of the program/script to run.
```
[service]
path = "/home/federicoponzi/dev/horust/example/service/first.sh"
```
* `command` = `string`: Will run the provided command using `sh` shell, like: `sh -c 'your command here'`.
```
[service]
command = "curl google.com"
```
* `wd` = `string`: Change the current working directory to the value of wd, before spawning the service.
```toml
[service]
wd = "/"
```

* `restart` = `always|on-failure|never`: Defines the restart strategy.
```
[service]
wd = "on-failure"
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
You can check the healthness of your system using an endpoint:
