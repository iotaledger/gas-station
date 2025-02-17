FROM rust:1.83-bullseye AS chef
WORKDIR iota
ARG GIT_REVISION
ENV GIT_REVISION=$GIT_REVISION
RUN apt-get update && apt-get install -y cmake clang

# Build and cache all dependencies.
FROM chef AS builder
WORKDIR /
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

ARG ENTRY_BINARY=iota-gas-station


# Production Image
FROM debian:bullseye-slim AS runtime
RUN apt-get update && apt-get install -y libjemalloc-dev ca-certificates

ARG ENTRY_BINARY=iota-gas-station
COPY --from=builder /target/release/${ENTRY_BINARY} /usr/local/bin/entrypoint

ARG BUILD_DATE
ARG GIT_REVISION
LABEL build-date=$BUILD_DATE
LABEL git-revision=$GIT_REVISION

ENTRYPOINT ["/usr/local/bin/entrypoint"]
