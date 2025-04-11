#!/bin/bash

IMAGE_NAME="iotaledger/gas-station-tool:latest"

docker run -it --rm \
      --name iota-gas-station-tool \
      -v $(pwd):/app \
      -w /app \
      -u $(id -u):$(id -g) \
      ${IMAGE_NAME} "$@"

