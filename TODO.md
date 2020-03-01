* Fix healthcheck to be real healthchecks.
* Implement RestartStrategies
* Add tests for http healthcheck.
* Per-service resource limits
* Parameter to force death if any service is incorrect.
* Reload configuration via SIGHUP (or another signal because as of now it can be run in a terminal).
* signal rewriting

## Long todo:
* Supervised services: use a pointer instead of list of services names (for example, for easier dependencies management.)
* Try to load config, if config is not deserializable, run the system with some sane defaults.
* Reduce size.
* Create another binary for getting the status of the services.
* Create another binary for validating the config file.
* Benchmark startup time
