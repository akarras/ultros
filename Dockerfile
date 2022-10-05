FROM rust:latest as builder

# Make a fake Rust app to keep a cached layer of compiled crates
# RUN USER=root cargo new app
# WORKDIR /usr/src/app
# COPY ./ultros/Cargo.toml ./ultros/Cargo.lock ./
# # Needs at least a main.rs file with a main function
# RUN mkdir src && echo "fn main(){}" > src/main.rs
# # Will build all dependent crates in release mode
# RUN --mount=type=cache,target=/usr/local/cargo/registry \
#     --mount=type=cache,target=/usr/src/app/target \
#     cargo build --release

# Copy the rest
COPY . .
# Build (install) the actual binaries
ENV RUSTFLAGS='-C target-cpu=native'
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/home/root/app/target \
    cargo install --path ultros

# Runtime image
FROM debian:bullseye-slim

# Run as "app" user
RUN useradd -ms /bin/bash app

USER app
WORKDIR /app

# Get compiled binaries from builder's cargo install directory
COPY --from=builder /usr/local/cargo/bin/ultros /app/ultros

# No CMD or ENTRYPOINT, see fly.toml with `cmd` override.