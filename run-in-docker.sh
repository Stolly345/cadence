#!/bin/sh
# Run a command in the Ubuntu 24.04 build image with this project mounted.
exec docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -v "$(dirname "$(readlink -f "$0")")":/w \
  -v cadence-cargo-registry:/opt/cargo/registry -w /w cadence-build "$@"
