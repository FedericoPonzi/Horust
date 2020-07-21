# Documentation
## Table of contents:
* [Service configuration](#service-configuration)
* [State machine](#state-machine)
* [Horust's configuration](#horust-configuration)
* [Single command](#single-command)
* [Plugins](#plugins)
* [Checking system status](#checking-system-status)

When starting horust, you can optionally specify where it should look for services and uses `/etc/horust/services` by default.

## Service configuration
This section describes all the possible options you can put in a service.toml file.
You should create one different service.toml for each command you want to run. 
A part from the `user` parameter, everything should work even with an unprivileged user.

### Main section
```toml
# name = "myname"
command = "/bin/bash -c 'echo hello world'"
start-delay = "2s"
start-after = ["another.toml", "second.toml"]
stdout = "STDOUT"
stderr = "/var/logs/hello_world_svc/stderr.log"
user = "root"
working-directory = "/tmp/"
```
* **`name` = `string`**: Name of the service. Optional, uses the filename by default.
* **`command` = `string`**: Specify a command to run, or a full path. You can also add arguments. If a full path is not provided, the binary will be searched using the $PATH env variable.
* **`start-after` = `[list<ServiceName>`**: Start after these other services. User their filename (e.g. `first.toml`).
If service `a` should start after service `b`, then `a` will be started as soon as `b` is considered Running or Finished. 
If `b` enters in a FinishedFailed state (finished in an unsuccessful manner), `a` might not start at all. 
* **`start-delay` = `time`**: Start this service with the specified delay. Check how to specify times [here](https://github.com/tailhook/humantime/blob/49f11fdc2a59746085d2457cb46bce204dec746a/src/duration.rs#L338) 
* **`stdout` = `STDOUT|STDERR|file-path`**: Redirect stdout of this service. STDOUT and STDERR are special strings, pointing to stdout and stderr respectively. Otherwise, a file path is a assumed.
* **`stderr` = `STDOUT|STDERR|file-path`**: Redirect stderr of this service. Read `stdout` above for a complete reference.
* **`user` = `uid|username`**: Will run this service as this user. Either an uid or a username (check it in /etc/passwd)
* **`working-directory` = `string`**: Will run this command in this directory.

#### Restart section
```toml
[restart]
strategy = "never"
backoff = "0s"
attempts = 0
```
* **`strategy` = `always|on-failure|never`**: Defines the restart strategy.

    * `always`: Failure or Success, it will be always restarted
    * `on-failure`: Only if it has failed. Please check the attempts parameter below.
    * `never`: It won't be restarted, no matter what's the exit status. Please check the attempts parameter below.

* **`backoff` = `string`**: Use this time before retrying restarting the service. 
* **`attempts` = `number`**: How many attempts to start the service before considering it as FinishedFailed. Default is 10.
Attempts are useful if your service is failing too quickly. If you're in a start-stop loop, this will put and end to it.
If a service has failed too quickly and attempts > 0, it will be restarted even if the strategy is `never`. 
And if the attempts are over, it won't never be restarted even if the restart policy is: On-Failure/ Always.

The delay between attempts is calculated as: `backoff * attempts_made + start-delay`. For instance, using:
* backoff = 1s
* attempts = 3
* start-delay = 1s"

Will wait 1 second and then start the service. If it doesn't start:
* 1st attempt will start after 1*1 + 1 = 2 seconds.
* 2nd attempt will start after 1*2 + 1 = 3 seconds.
* 3th and last attempt will start after 1*3 +1 = 4 seconds. 

If the attempts are over, then the service will be considered FailedFinished and won't be restarted.
The attempt count is reset as soon as the service's state changes to running.
This state change is driven by the healthcheck component, and a service with no healthcheck will be considered as Healthy and it will
immediately pass to the running state.

### Healthiness Check
```toml
[healthiness]
http-endpoint = "http://localhost:8080/healthcheck"
file-path = "/var/myservice/up"
```
 * **`http-endpoint` = `<http endpoint>`**: It will send an HEAD request to the specified http endpoint. 200 means the service is healthy, otherwise it will change the status to failure.
    This requires horust to be built with the `http-healthcheck` feature (included by default).
 * **`file-path` = `/path/to/file`**: Before running the service, it will remove this file if it exists. Then, as soon as this file is created, the service will be considered running. 
 * You can check the healthiness of your system using an http endpoint or a flag file.
 * You can use the enforce dependency to kill every dependent system.

### Failure section
```toml
[failure]
successful-exit-code = [ 0, 1, 255]
strategy = "ignore"
```
* **`successful-exit-code` = `[\<int>]`**: A comma separated list of exit code. 
Usually a program is considered failed if its exit code is different than zero. But not all fails are the same.
By using this parameter, you can specify which exit codes will make this service considered as failed.

* **`strategy` = `shutdown|kill-dependents|ignore`**': We might want to kill the whole system, or part of it, if some service fails. Default: `ignore`

     * `kill-dependents`: Dependents are all the services start after this one. So if service `b` has service `a` in its `start-after` section,
        and `a` has strategy=kill-dependents, then b will be stopped if `a` fails.
     * `shutdown`: It will kill all the services.

### Environment section
```toml
[environment]
keep-env = false
re-export = [ "PATH", "DB_PASS"]
additional = { key = "value"} 
```
* **`keep-env` = `bool`**: default: true. Pass over all the environment variables.
Regardless the value of keep-env, the following keys will be updated / defined:
* `USER`
* `HOSTNAME`
* `HOME`
* `PATH`
Use `re-export` for keeping them.
* **`re-export` = `[\<string>]`**: Environment variables to keep and re-export.
This is useful for fine-grained exports or if you want for example to rexport the `PATH`.
* **`additional` = `{ key = <string> }`**: Defined as key-values, other environment variables to use.

### Termination section
```toml
[termination]
signal = "TERM"
wait = "10s"
die-if-failed = ["db.toml"]
```
* **`signal` = `"TERM|HUP|INT|QUIT|USR1|USR2"`**: The _friendly_ signal used for shutting down the process.
* **`wait` = `"time"`**: How much time to wait before sending a SIGKILL after `signal` has been sent.
* **`die-if-failed` = `["<service-name>"]`**: As soon as any of the services defined in this the array fails, this service will be terminated as well.

---

## State machine
[![State machine](https://github.com/FedericoPonzi/Horust/raw/master/res/state-machine.png)](https://github.com/FedericoPonzi/Horust/raw/master/res/state-machine.png)

You can compile this on https://state-machine-cat.js.org/
```
initial => Initial : "Will eventually be run";
Initial => Starting : "All dependencies are running, a thread has spawned and will run the fork/exec the process";
Initial => Finished : "System shutdown before service had a chance to run (Kill Event)"; 
Starting => Started : "The service has a pid";
Started => Running : "The service has met healthiness policy";
Started => Failed : "Service cannot be started";
Started => Success : "Service finished very quickly";
Failed => FinishedFailed : "Restart policy";
Started => InKilling : "Received a Kill event";
InKilling => Finished : "Successfully killed";
InKilling => FinishedFailed : "Forcefully killed (SIGKILL)";
Running => Failed  : "Exit status is not successful";
Running => Success  : "Exit status == 0";
Running => InKilling: "Received a Kill event";
Success => Initial : "Restart policy applied";
Success => Finished : "Based on restart policy";
Failed => Initial : "restart = always|on-failure";
```

## Horust's configuration
Horust can be configured by using the following parameters:
```toml
# Default time to wait after sending a `sigterm` to a process before sending a SIGKILL.
unsuccessful-exit-finished-failed = true
```
All the parameters can be passed via the cli (use `horust --help`) or via a config file.
The default path for the config file is `/etc/horust/horust.toml`.

## Single command
It's already supported, but I think it needs some love.
You can wrap a single command with horust by running:
``` bash
./horust -- bash /tmp/myscript.sh
```
This is equivalent to running a single service defined as:
```
command= "bash /tmp/myscript.sh"
```
This will run the specified command as a one shot service, so it won't be restarted after exiting.
Commands have precedence over services, so if you specify both a command and a services-path, the command will be executed and the services path is ignored.

## Plugins
WIP. Horust works via message passing, so it should be fairly easy to have additional components connected to its bus.
But I'm not sure at this time, if there is the need for this.

## Checking system status
WIP: https://github.com/FedericoPonzi/Horust/issues/31
The idea is to create another binary, which will somehow report the system status. A `systemctl` for Horust.