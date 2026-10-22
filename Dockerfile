FROM rust:1.98.1-bookworm AS build
WORKDIR /build
ENV CARGO_BUILD_JOBS=2
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY meada-core/ meada-core/
COPY src/ src/
COPY examples/golden_diff.rs examples/golden_diff.rs
COPY tests/golden/schema.txt tests/golden/schema.txt
ARG FEATURES=""
RUN cargo build --release --locked --features "$FEATURES"

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install --no-install-recommends -y git ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 --user-group meada
COPY --from=build /build/target/release/meada /usr/local/bin/meada
ARG VERSION="0.1.0"
LABEL org.opencontainers.image.source="https://github.com/ClaudioMascaro/meada" \
      org.opencontainers.image.version="$VERSION" \
      org.opencontainers.image.licenses="Apache-2.0"
USER meada
ENV HOME=/home/meada
WORKDIR /home/meada
ENTRYPOINT ["meada"]
