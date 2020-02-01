* Supervised services: use a pointer instead of list of services names

* Spawn all the processes with a new process group (to ease the shutdown via killpg)
* Reload configuration via SIGHUP (or another signal because as of now it can be run in a terminal).
* Create another binary for getting the status of the services.
* healthchecks
* Per-service resource limits
* Benchmark startup time

## Long todo:
* Try to load config, if config is not deserializable, run the system with some sane defaults.
