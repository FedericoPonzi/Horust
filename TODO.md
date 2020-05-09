* Count all time the unhealthy events, if threshold is passed and 
    service is in started then stop it.
* Parameter for redirecting stdout / stderr to files

## Long todo:
* Parameter "start-if-failed"
* Create another binary for getting the status of the services.
    * Store timestamp when starting a new process (for knowing uptime)
* Create another binary for validating the config file.
* Setup build and release on github
    * Include git hash in version