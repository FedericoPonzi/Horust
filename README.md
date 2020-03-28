[<img src="https://github.com/FedericoPonzi/Horust/raw/master/res/horust-logo.png" width="300" align="center">](https://github.com/FedericoPonzi/Horust/raw/master/res/horust-logo.png)

[![GHA Build Status](https://github.com/FedericoPonzi/horust/workflows/CI/badge.svg)](https://github.com/FedericoPonzi/horust/actions?query=workflow%3ACI) [![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

[Horust](https://github.com/FedericoPonzi/Horust) is a supervisor system written in rust and designed to be run in containers. 

# Table of contents
* [Goals](#goals)
* [Status](#status)
* [Usage](#usage)
* [Contributing](#contributing)
* [License](#license)

## Goals
* Supervision: A feature full supervision system, designed (but not limited) to be used in containers.
* Init system: Use Horust as your init system.
* Understandability: The code should be easy to understand and easy to modify.
* Rock solid: You should be able to trust your favorite egyptian God.

## Status
At this point, this should be considered Alpha software.

## Usage
Let's go through a simple example usage. We will create a website healthchecker using horust and a python script.
1. Create a directory: `mkdir -p /etc/horust/services`
2. Create your first service: `/etc/horust/services/healthchecker.toml`
```toml
command = "/tmp/healthcheck.py"
start-delay = "10s"
[restart]
strategy = "always"
``` 
There are many supported properties for your service file, but they are all optional. This is as minimal as we can get.
As soon as we run horust, this service will be read and the command run. As soon as the service is over it won't be restarted, and horust will exit.

3. Create a new file called `healthcheck.py` and in tmp:
```
#!/usr/bin/python3
import urllib.request
import sys
req = urllib.request.Request("https://www.google.com", method="HEAD")
resp = urllib.request.urlopen(req)
if resp.status == 200:
    sys.exit(0)
else:
    sys.exit(1)
```
Add execution permissions to it: `chmod +x /tmp/healthcheck.py`.

4. Run horust: "./horust". By default it will search services inside the `/etc/horust/services` folder.

Now every 10 seconds, this will send an http head request to google.it. If the response is different than 200, then there is an issue!

In this case, we're just exiting with a different exit code. But in real life, you could trigger other actions.
Use ctrl+c for stopping horust. Horust will send a SIGTERM signal to all the running services, and if it doesn't hear back for a while, it will terminate them via a SIGKILL.

---

Check the [documentation](https://github.com/FedericoPonzi/Horust/blob/master/DOCUMENTATION.md) for a complete reference of the options available on the service config file.

> You can also bootstrap the creation of a new service, by using `horust --sample-service > new_service.toml`.

```toml
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
http-endpoint = "http://localhost:8080/healthcheck"
file-path = "/var/myservice/up"

[failure]
successful-exit-code = [ 0, 1, 255]
strategy = "ignore"

[termination]
signal = "TERM"
wait = "10s"
die-if-failed = ["db.toml"]
```

## Contributing
Thanks for considering contributing to horust! For getting started have a look on [CONTRIBUTING.md](https://github.com/FedericoPonzi/Horust/blob/master/DEVELOPMENT.md).

## License
Horust is provided under the MIT license. Please read the attached [license](https://github.com/FedericoPonzi/horust/blob/master/LICENSE) file.
