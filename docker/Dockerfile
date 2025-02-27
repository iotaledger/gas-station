FROM --platform=$BUILDPLATFORM rust:1.84-bullseye AS builder

ARG TARGETPLATFORM
ARG BUILDPLATFORM

ARG ENTRY_BINARY=iota-gas-station
ARG GIT_REVISION

ENV GIT_REVISION=$GIT_REVISION

WORKDIR /app

RUN apt-get update && apt-get install -y \
  cmake \
  clang \
  gcc-aarch64-linux-gnu \
  && rm -rf /var/lib/apt/lists/*

# Make sure we have both x86_64 *and* aarch64 targets installed in Rust
#  - x86_64-unknown-linux-gnu is installed by default
RUN rustup target add aarch64-unknown-linux-gnu

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN if [ "$TARGETPLATFORM" = "linux/arm64" ]; then \
  echo "Building for ARM64 (aarch64-unknown-linux-gnu)"; \
  cargo build --release --target aarch64-unknown-linux-gnu; \
  else \
  echo "Building for AMD64 (x86_64-unknown-linux-gnu)"; \
  cargo build --release; \
  fi


FROM ${TARGETPLATFORM}/debian:bullseye-slim AS runtime

ARG TARGETPLATFORM
ARG ENTRY_BINARY
ARG BUILD_DATE
ARG GIT_REVISION

LABEL build-date=$BUILD_DATE
LABEL git-revision=$GIT_REVISION

# Install runtime dependencies
RUN apt-get update \
  && apt-get install -y libjemalloc-dev ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/aarch64-unknown-linux-gnu/release/$ENTRY_BINARY /tmp/entrypoint-arm64
COPY --from=builder /app/target/release/$ENTRY_BINARY                   /tmp/entrypoint-amd64

RUN if [ "$TARGETPLATFORM" = "linux/arm64" ]; then \
  echo "Using ARM64 binary." \
  && cp /tmp/entrypoint-arm64 /usr/local/bin/entrypoint; \
  else \
  echo "Using AMD64 binary." \
  && cp /tmp/entrypoint-amd64 /usr/local/bin/entrypoint; \
  fi \
  && chmod +x /usr/local/bin/entrypoint \
  && rm -f /tmp/entrypoint-arm64 /tmp/entrypoint-amd64 2>/dev/null || true

ENTRYPOINT ["/usr/local/bin/entrypoint"]
