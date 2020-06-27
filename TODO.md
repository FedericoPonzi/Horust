* Count all time the unhealthy events, if threshold is passed and 
    service is in started then stop it.
* Better loggig facility. Using a file as stdio it's not the best.
* Send SIGKILL to whole processgroup when killing a service

## Long todo:
* Parameter "start-if-failed"
* Create another binary for getting the status of the services.
    * Store timestamp when starting a new process (for knowing uptime)
    * Send ServiceAdded event and handle runtime services addition 
    * Services config file validation
* Setup build and release on github
    * Include git hash in version