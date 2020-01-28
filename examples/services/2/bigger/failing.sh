#!/bin/bash
echo "[failing.sh] pwd is: " $(pwd)
echo "Args:" "$@"
sleep 1
exit 1
#ls -l >> /tmp/out.txt