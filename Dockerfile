FROM rustlang/rust:nightly-bullseye as builder
# Install system packages
RUN apt update; apt upgrade -y 
RUN apt install -y libfreetype6 libfreetype6-dev cmake
# Configure rustup
RUN rustup component add rust-src
RUN rustup target add wasm32-unknown-unknown
RUN cargo install --locked cargo-leptos
RUN rustup target add aarch64-unknown-linux-gnu
RUN rustup update
# Thank you benwis https://github.com/benwis/benwis_leptos/blob/main/Dockerfile
RUN mkdir -p /app
WORKDIR /app
COPY . .
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
env PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-linux-gnu/
RUN cargo leptos --manifest-path=./Cargo.toml build --release -vv

FROM rustlang/rust:nightly-bullseye as runner
COPY --from=builder /app/target/release/ultros /app/
COPY --from=builder /app/target/site /app/site
COPY --from=builder /app/Cargo.toml /app/
# copy font into local font dirs
COPY --from=builder /app/ultros/static/*.ttf /usr/local/share/fonts
WORKDIR /app
ENV RUST_LOG="info"
ENV LEPTOS_OUTPUT_NAME="ultros"
ENV LEPTOS_ENVIRONMENT="production"
ENV LEPTOS_SITE_ADDR="0.0.0.0:8080"
ENV LEPTOS_SITE_ROOT="site"
EXPOSE 8080
CMD ["/app/ultros"]