# Keep tool pins aligned with ui/.nvmrc and ui/scripts/wasm.mjs.
FROM --platform=$BUILDPLATFORM node:22.23.1-bookworm@sha256:5647be709086c696ff32edaaf1c70cd26d1da6ab2b39c32f3c7b4c4a31957e37 AS node
FROM --platform=$BUILDPLATFORM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS frontend
COPY --from=node /usr/local/bin/node /usr/local/bin/node
COPY --from=node /usr/local/lib/node_modules/npm /usr/local/lib/node_modules/npm
ARG BUILDARCH
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && case "$BUILDARCH" in \
      amd64) arch=x86_64; sha=c09f971ecaed9a2efc80fdcea7a00ef6b53c7fadc8c57d1f61b53a6aa66b668a ;; \
      arm64) arch=aarch64; sha=e17ef0806381c3a0acb9c9ddad643a49facaa5a2ecf657a421d4d8f3357a24b7 ;; \
      *) echo "unsupported frontend build architecture: $BUILDARCH" >&2; exit 1 ;; \
    esac \
    && curl -sSfL -o /tmp/wasm-pack.tar.gz "https://github.com/wasm-bindgen/wasm-pack/releases/download/v0.15.0/wasm-pack-v0.15.0-$arch-unknown-linux-musl.tar.gz" \
    && echo "$sha  /tmp/wasm-pack.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/wasm-pack.tar.gz -C /tmp \
    && install -m 755 "/tmp/wasm-pack-v0.15.0-$arch-unknown-linux-musl/wasm-pack" /usr/local/bin/wasm-pack \
    && rm -rf /tmp/wasm-pack* \
    && rustup target add wasm32-unknown-unknown
WORKDIR /build
ENV CARGO_BUILD_JOBS=2
COPY ui/package.json ui/package-lock.json ui/
RUN cd ui && npm ci
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY meada-core/ meada-core/
COPY src/ src/
COPY examples/golden_diff.rs examples/golden_diff.rs
COPY tests/golden/schema.txt tests/golden/schema.txt
COPY ui/ ui/
RUN cd ui && npm run wasm && npm run build && npm run check:version

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

FROM debian:bookworm-slim AS base
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

FROM base AS cloud
COPY --from=frontend /build/ui/dist/ /usr/share/meada/ui/

# The default target does not depend on Node or the frontend stage.
FROM base AS runtime
