* Count all time the unhealthy events, if threshold is passed and 
    service is in started then stop it.
* Better logging facility. Using a file as stdio it's not the best.
* Send SIGKILL to whole processgroup when killing a service
* Stack all the things!

## Long todo:
* Parameter "start-if-failed". Might be worth it to generalize start-if = [ServiceName: Status]?
* Create another binary for getting the status of the services:
    * Send ServiceAdded event and handle runtime services addition 
    * Services config file validation
    * Store timestamp when starting a new process (for knowing uptime)