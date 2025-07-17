#!/bin/sh
# Copyright (c) Mysten Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# fast fail.
set -e

DIR="$( cd "$( dirname "$0" )" && pwd )"
REPO_ROOT="$(git rev-parse --show-toplevel)"
DOCKERFILE="$DIR/Dockerfile"
GIT_REVISION="$(git describe --always --dirty --exclude '*')"
BUILD_DATE="$(date -u +'%Y-%m-%d')"
ENTRY_BINARY="${ENTRY_BINARY:-iota-gas-station}"


echo
echo "Building iota-gas-station docker image"
echo "Dockerfile: \t$DOCKERFILE"
echo "docker context: $REPO_ROOT"
echo "build date: \t$BUILD_DATE"
echo "git revision: \t$GIT_REVISION"
echo "binary: \t$ENTRY_BINARY"
echo

docker buildx build -f "$DOCKERFILE" "$REPO_ROOT" \
	--build-arg GIT_REVISION="$GIT_REVISION" \
	--build-arg BUILD_DATE="$BUILD_DATE" \
	--build-arg ENTRY_BINARY="$ENTRY_BINARY" \
	"$@"
