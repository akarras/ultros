# syntax=docker/dockerfile:1.7

# ---- Base toolchain layer ----------------------------------------------------
# Shared by chef/planner/builder so the toolchain install is cached once.
FROM rust:bookworm AS chef
ENV PATH="/root/.cargo/bin:${PATH}" \
    CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        cmake \
        curl \
        fontconfig \
        git \
        libfontconfig1-dev \
        libfreetype6 \
        libfreetype6-dev \
        libssl-dev \
        pkg-config \
        sudo \
    && rm -rf /var/lib/apt/lists/*
# Toolchain (channel pinned by ./rust-toolchain). Pre-installing rust-src and the
# wasm32 target here so cargo-chef cook can build wasm deps without re-fetching.
COPY ./rust-toolchain ./rust-toolchain
RUN rustup show && rustup component add rust-src && rustup target add wasm32-unknown-unknown
# cargo-binstall + cargo-chef + cargo-leptos + wasm-bindgen-cli in one layer.
RUN curl -L --proto '=https' --tlsv1.2 -sSf \
        https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh \
        | bash \
    && cargo binstall -y \
        cargo-chef \
        cargo-leptos@0.3 \
        wasm-bindgen-cli@0.2.122
WORKDIR /app

# ---- Recipe: extract dependency graph for cache-friendly rebuilds ------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---- Builder: cook deps first (cached), then compile the project -------------
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Warm the dependency cache for BOTH targets cargo-leptos will use.
# Scoping with `-p` is REQUIRED: without it, `chef cook` tries to build every
# workspace member for the given target — and the workspace contains both the
# native server (`ultros`, pulls tokio="full" → mio) and the WASM client
# (`ultros-client`, cdylib for wasm32). Cross-compiling the server crate for
# wasm32 fails on mio; building the WASM cdylib for native x86_64 fails too.
#  - bin-package = "ultros"        → server-release profile, native
#  - lib-package = "ultros-client" → release profile, wasm32-unknown-unknown
# Edits to source code below this line won't invalidate these layers.
RUN cargo chef cook --profile server-release -p ultros --recipe-path recipe.json
RUN cargo chef cook --release --target wasm32-unknown-unknown -p ultros-client --recipe-path recipe.json
# Now the actual source.
COPY . .
ENV WASM_BINDGEN_WEAKREF=1
RUN cargo leptos --manifest-path=./Cargo.toml build --release -vv
# Split debug info: keep an unstripped copy for CI to upload to GlitchTip,
# strip the production binary. objcopy is in binutils (transitive via
# build-essential). The GNU build-id NOTE survives stripping and is the
# correlation key GlitchTip uses to match the stripped runtime binary against
# the uploaded debug file.
#
# zlib compression of debug sections is the well-trodden path (DWARF GABI
# SHF_COMPRESSED, decade-old support in everything that reads ELF DWARF).
# zstd is also valid per spec but newer; sticking with zlib avoids any chance
# of glitchtip-cli's symbolic-based parser tripping on a too-modern format.
# Roughly 3–4× shrink on .debug_* sections vs uncompressed; uncompressed
# upload would otherwise be ~250–300MB per push.
RUN cp /app/target/server-release/ultros /app/target/server-release/ultros.unstripped \
    && objcopy --compress-debug-sections=zlib /app/target/server-release/ultros.unstripped \
    && objcopy --strip-debug --strip-unneeded /app/target/server-release/ultros

# ---- Debug-files artifact (export-only) --------------------------------------
# Tiny stage holding only the unstripped binary. Never part of any pushed image.
# CI builds this with `--target debug-files --output type=local,dest=./debug`
# and uploads via `glitchtip-cli debug-files upload ./debug` for stack-trace
# symbolication. Local `docker build` ignores this stage entirely.
#
# Placed before `runner` so the default (no --target) build still produces the
# runtime image, not this artifact.
FROM scratch AS debug-files
COPY --from=builder /app/target/server-release/ultros.unstripped /ultros.unstripped

# ---- Runtime image -----------------------------------------------------------
FROM debian:bookworm-slim AS runner
ENV DEBIAN_FRONTEND=noninteractive \
    RUST_LOG="info" \
    LEPTOS_OUTPUT_NAME="ultros" \
    LEPTOS_ENVIRONMENT="production" \
    LEPTOS_SITE_ADDR="0.0.0.0:8080" \
    LEPTOS_SITE_ROOT="site"
# Minimal runtime deps: TLS roots, freetype + fontconfig for plotters/resvg rendering.
# No `apt upgrade` — keeps builds reproducible against the pinned base image.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        fontconfig \
        libfontconfig1 \
        libfreetype6 \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# cargo-leptos writes the bin to target/<bin-profile-release>/ when that's set.
COPY --from=builder /app/target/server-release/ultros /app/ultros
COPY --from=builder /app/target/site /app/site
COPY --from=builder /app/Cargo.toml /app/Cargo.toml
# Fonts used by plotters/resvg at runtime. Same files are also in /app/site for
# the client; keeping the system install lets fontconfig find them server-side.
RUN mkdir -p /usr/local/share/fonts
COPY --from=builder /app/ultros/static/*.ttf /usr/local/share/fonts/
RUN fc-cache -f
RUN mkdir -p /app/analyzer-data
VOLUME ["/app/analyzer-data"]
EXPOSE 8080
CMD ["/app/ultros"]
