FROM --platform=$BUILDPLATFORM tonistiigi/xx AS xx

FROM --platform=$BUILDPLATFORM  rust:1.85-bullseye AS chef

# setup xx
# copy xx scripts to your build stage
COPY --from=xx / /
# export TARGETPLATFORM (or other TARGET*)
ARG TARGETPLATFORM
# you can now call xx-* commands
RUN xx-info env

WORKDIR /iota

ARG GIT_REVISION
ENV GIT_REVISION=$GIT_REVISION

# This is a workaround for the following the issue related to the libc-bin package
RUN rm /var/lib/dpkg/info/libc-bin.*

RUN apt-get update && apt-get install -y clang lld

# Build and cache all dependencies.
FROM chef AS builder
WORKDIR /iota

COPY Cargo.toml ./
COPY src ./src
ARG ENTRY_BINARY=iota-gas-station

RUN xx-cargo build --release --target-dir ./build
RUN xx-verify "./build/$(xx-cargo --print-target-triple)/release/${ENTRY_BINARY}"

# move binary to a location we can access it more easily in `FROM` command
RUN mv "./build/$(xx-cargo --print-target-triple)/release/${ENTRY_BINARY}" ./build/entrypoint

# Production Image
FROM debian:bullseye-slim AS runtime
RUN apt-get update && apt-get install -y libjemalloc-dev ca-certificates

COPY --from=builder "/iota/build/entrypoint" /usr/local/bin/entrypoint

ARG BUILD_DATE
ARG GIT_REVISION
LABEL build-date=$BUILD_DATE
LABEL git-revision=$GIT_REVISION

ENTRYPOINT ["/usr/local/bin/entrypoint"]