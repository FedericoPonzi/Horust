* Add tests for http healthcheck.
* Parameter to force death if any service is incorrect.
* If running via command, should just proxy signals instead of shutting down the system.

## Long todo:
* Try to load config, if config is not deserializable, run the system with some sane defaults.
* Per-service resource limits
* Create another binary for getting the status of the services.
    * Store timestamp when starting a new process (for knowing uptime)
* Create another binary for validating the config file.
* Setup build and release on github
    * Include git hash in version