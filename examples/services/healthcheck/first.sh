#!/bin/bash
echo "[First.sh] pwd is: " $(pwd)
echo "Args:" "$@"
sleep 1
touch /tmp/horust-ready
sleep 5
echo "Bye!"