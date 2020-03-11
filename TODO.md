* "One shot" services aka init services aka one shot calls.
* Permission changer format.
* Add tests for http healthcheck.
* Per-service resource limits
* Parameter to force death if any service is incorrect.
* after loaded services, check if dependant services exists
* Store timestamp when starting a new process (for knowing uptime)
* should print something if no services are found.
* If running via command, should just proxy signals instead of shutting down the system.

## Long todo:
* Supervised services: use a pointer instead of list of services names (for example, for easier dependencies management.)
* Try to load config, if config is not deserializable, run the system with some sane defaults.
* Reduce size.
* Create another binary for getting the status of the services.
* Create another binary for validating the config file.
* Benchmark startup time
* Setup build and release on github
    * Include git hash in version

On service statuses:
* killed? After a sigkill
* FinishedFailed? A failed process which won't be restarted vs Failed which has failed and will be restarted.
