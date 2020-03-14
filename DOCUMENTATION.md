# Documentation
Since the README it's growing too much long, and it will likely increase as features are developed, for now the complete docs will be stored here.

### Service section
* **`name` = `string`**: Name of the service. If not defined, it will use the filename instead.
* **`command` = `string`**: Specify a command to run, or a full path. You can also add arguments. If a full path is not provided, the binary will be searched using the $PATH env variable.
* **`working_directory` = `string`**: will use this value as current working directory for the service.
* **`user` = `uid|username`**: Will run this service as this user. Either an uid or a username (check it in /etc/passwd)
* **`start-after` = `list<string>`**: Start after these other services. User their filename.
* **`start-delay` = `time`**: Start this service with the specified delay. Check how to specify times [here](https://github.com/tailhook/humantime/blob/49f11fdc2a59746085d2457cb46bce204dec746a/src/duration.rs#L338) 

#### Restart section
* **`strategy` = `always|on-failure|never`**: Defines the restart strategy.
* **`backoff`** = `string`: Use this time before retrying restarting the service. 
* **`attempts`** = `number`: How many attempts before considering the service as Failed.

The delay between attempts is calculated as: `backoff * attempts_made + start-delay`. For instance, using:
* backoff = 1s
* attempts = 3
* start-delay = 1s"

Will wait 1 second and then start the service. If it doesn't start:
* 1st attempt will start after 1*1 + 1 = 2 seconds.
* 2nd attempt will start after 1*2 + 1 = 3 seconds.
* 3th and last attempt will start after 1*3 +1 = 4 seconds. 

If this fails, the service will be considered FailedFinished and won't be restarted.

The attempt count is reset as soon as the service's state changes from starting to running (healthcheck passes).

#### Readiness
* **`readiness` = `health`**: If not present, the service will be considered ready as soon as has been spawned. Otherwise, use:
    * **`health`**: Use the same strategy defined in the health configuration, 
    * **`custom command`**: If the custom command is successful then your service is ready.

### Healthiness Check
 * You can check the healthiness of your system using an http endpoint.
 * You can use the enforce dependency to kill every dependent system.

### Signal rewriting
Horust allows rewriting incoming signals before proxying them. This is useful in cases where you have a Docker supervisor (like Mesos or Kubernetes) which always sends a standard signal (e.g. SIGTERM). Some apps require a different stop signal in order to do graceful cleanup.
For example, to rewrite the signal SIGTERM (number 15) to SIGQUIT (number 3) add a rewrite property on your service file.
To drop a signal entirely, you can rewrite it to the special number 0.

### Failure section
```toml
[failure]
successfull_exit_code = ["0", "1", "255"]
strategy = "ignore"
```
* **exit_code = \<int>[,\<int>]**: A comma separated list of exit code. Usually a program is considered failed if its exit code is different than zero. But not all fails are the same. By using this parameter, you can specify which exit codes will make this service considered failed.
* **strategy = `shutdown|kill-dependents|ignore`**': We might want to kill the whole system, or part of it, if some service fails. By default the failure won't trigger anything.

### Termination section
```toml
[termination]
signal = "TERM"
wait = "10s"
```
* **signal** = **"TERM|HUP|INT|QUIT|KILL|USR1|USR2"** signal used for shutting down the process.
* **wait** = **"time"** how much time to wait before sending a SIGKILL after `signal` has been sent.

---
## Plugins
WIP