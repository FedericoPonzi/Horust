# Documentation
This document describe all the possible options you can put in a service.toml file.
You should create one different service.toml for each command you want to run. 

When starting horust, you can optionally specify where it should look for services and uses `/etc/horust/services` by default.
A part from the `user` parameter, everything should work even with an unprivileged user.

### Main section
```toml
# name = "myname"
command = "/bin/bash -c 'echo hello world'"
working-directory = "/tmp/"
start-delay = "2s"
start-after = ["another.toml", "second.toml"]
user = "root"
```
* **`name` = `string`**: Name of the service. Optional, uses the filename by default.
* **`command` = `string`**: Specify a command to run, or a full path. You can also add arguments. If a full path is not provided, the binary will be searched using the $PATH env variable.
* **`working-directory` = `string`**: Will run this command in this directory.
* **`start-after` = `[list<ServiceName>`**: Start after these other services. User their filename (e.g. `first.toml`).
If service `a` should start after service `b`, then `a` will be started as soon as `b` is considered Running or Finished. 
If `b` enters in a FinishedFailed state (finished in an unsuccessful manner), `a` might not start at all. 
* **`start-delay` = `time`**: Start this service with the specified delay. Check how to specify times [here](https://github.com/tailhook/humantime/blob/49f11fdc2a59746085d2457cb46bce204dec746a/src/duration.rs#L338) 
* **`user` = `uid|username`**: Will run this service as this user. Either an uid or a username (check it in /etc/passwd)

#### Restart section
```toml
[restart]
strategy = "never"
backoff = "0s"
attempts = 0
```
* **`strategy` = `always|on-failure|never`**: Defines the restart strategy.
    * Always: Failure or Success, it will be always restarted
    * `on-failure`: Only if it has failed. Please check the attempts parameter below.
    * `never`: It won't be restarted, no matter what's the exit status. Please check the attempts parameter below.
* **`backoff`** = `string`: Use this time before retrying restarting the service. 
* **`attempts`** = `number`: How many attempts before considering the service as FinishedFailed. Default is 10.
Attempts are useful if your service is failing too quickly. If you're in a start-stop loop, this will put and end to it.
If a service has failed too quickly, it will be restarted even if the policy is `never`. 
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
 * **http** = 
 * You can check the healthiness of your system using an http endpoint or a flag file.
 * You can use the enforce dependency to kill every dependent system.

### Failure section
```toml
[failure]
successful-exit-code = [ 0, 1, 255]
strategy = "ignore"
```
* **successful-exit-code = [\<int>]**: A comma separated list of exit code. 
Usually a program is considered failed if its exit code is different than zero. But not all fails are the same.
By using this parameter, you can specify which exit codes will make this service considered as failed.

* **strategy = `shutdown|kill-dependents|ignore`**': We might want to kill the whole system, or part of it, if some service fails. Default: `ignore` 
 * `kill-dependents`: Dependents are all the services start after this one. So if service `b` has service `a` in its `start-after` section,
    and `a` has strategy=kill-dependents, then b will be stopped if `a` fails.
 * `shutdown`: It will kill all the services.

### Termination section
```toml
[termination]
signal = "TERM"
wait = "10s"
```
* **signal** = **"TERM|HUP|INT|QUIT|KILL|USR1|USR2"**: The _friendly_ signal used for shutting down the process.
* **wait** = **"time"**: How much time to wait before sending a SIGKILL after `signal` has been sent.

---

## Single command
WIP.

## Plugins
WIP. Horust works via events, so it should be fairly easy to have additional components connected to the bus.
## Check horust status
WIP. Feel free to contribute.
## Check services status
WIP. Feel free to contribute.
