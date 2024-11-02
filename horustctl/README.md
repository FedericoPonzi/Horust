## Horustctl

A command line interface to interact with an horust process. It works using Unix Domain Sockets.
Each horust process will create a new uds socket inside /var/run/horust/horust-<pid>.sock folder (can be configured).

You can use horustctl to interact with your horust process. The communication is handled by the `commands` crate. They
exchange protobuf messages.

Supported commands:

* status [servicename]: get the status of your service `servicename`. If not specified, it will return the status for
  all services.
* change <servicename> <newstatus>: can be used to change the status of `servicename`.
  Supported `newstatus` options are start, stop.
