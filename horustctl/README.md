## Horustctl

A command line interface to interact with an horust process. It works using Unix Domain Sockets.
Each horust process will create a new uds socket inside /var/run/horust/horust-<pid>.sock folder (can be configured).

You can use horustctl to interact with your horust process. The communication is handled by the `commands` crate. They
exchange protobuf messages.

Supported commands:

* `status [service_name]`: get the status of a service. If `service_name` is omitted, returns the status of all services.
* `start <service_name>`: start a stopped service.
* `stop <service_name>`: stop a running service.
* `restart <service_name>`: restart a service (stop then start). Internally reuses the stop and start operations.
* `reload`: reload service directories to pick up new service definitions added at runtime.
