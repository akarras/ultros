FROM rust:latest AS chef 
# We only pay the installation cost once, 
# it will be cached from the second build onwards
RUN cargo install cargo-chef 
WORKDIR app

FROM chef AS planner
COPY . .
ENV RUSTFLAGS='-C target-cpu=native'
RUN cargo chef prepare  --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin ultros

# We do not need the Rust toolchain to run the binary!
FROM debian:bullseye-slim AS runtime
RUN useradd -ms /bin/bash app
USER app
WORKDIR /app

COPY --from=builder /app/target/release/ultros /app/ultros


# No CMD or ENTRYPOINT, see fly.toml with `cmd` override.