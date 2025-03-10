FROM --platform=$BUILDPLATFORM  rust:1.85-bullseye AS chef

WORKDIR /iota

ARG GIT_REVISION
ENV GIT_REVISION=$GIT_REVISION

# This is a workaround for the following the issue related to the libc-bin package
RUN rm /var/lib/dpkg/info/libc-bin.*

RUN apt-get clean
RUN apt-get update && apt-get install -y cmake clang lld


# Build and cache all dependencies.
FROM chef AS builder
WORKDIR /iota

# Configure Rust to use clang and lld as the linker
RUN mkdir -p ./.cargo && \
  echo "[target.x86_64-unknown-linux-gnu]\nlinker = \"clang\"\nrustflags = [\"-C\", \"link-arg=-fuse-ld=lld\"]" > ./.cargo/config.toml

COPY Cargo.toml ./
COPY src ./src

RUN cargo build --release


# Production Image
FROM debian:bullseye-slim AS runtime
RUN apt-get update && apt-get install -y libjemalloc-dev ca-certificates

ARG ENTRY_BINARY=iota-gas-station
COPY --from=builder /iota/target/release/${ENTRY_BINARY} /usr/local/bin/entrypoint

ARG BUILD_DATE
ARG GIT_REVISION
LABEL build-date=$BUILD_DATE
LABEL git-revision=$GIT_REVISION

ENTRYPOINT ["/usr/local/bin/entrypoint"]
