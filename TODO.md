* Kill all process when sigterm is received: https://www.win.tue.nl/~aeb/linux/lk/lk-10.html
* Spawn all the processes with a new process group (to ease the shutdown via killpg)
* Reload configuration via SIGHUP (or another signal because as of now it can be run in a terminal).
* Create another binary for getting the status of the services.
* healthchecks
* Per-service resource limits