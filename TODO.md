* Add tests for http healthcheck.
* Reload configuration via SIGHUP (or another signal because as of now it can be run in a terminal).
* signal rewriting
* Per-service resource limits
* Parameter to force death if any service is incorrect.
* Implement RestartStrategies

## Long todo:
* Supervised services: use a pointer instead of list of services names (for example, for easier dependencies management.)
* Try to load config, if config is not deserializable, run the system with some sane defaults.
* Reduce size.
* Create another binary for getting the status of the services.
* Benchmark startup time
