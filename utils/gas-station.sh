#!/bin/env bash

IMAGE_NAME="iotaledger/gas-station:latest"

docker run --init -it --rm \
      --name iota-gas-station \
      --network host \
      -v $(pwd):/app \
      -w /app -u $(id -u):$(id -g) \
      -e GAS_STATION_AUTH="$GAS_STATION_AUTH" \
      ${IMAGE_NAME} "$@"
