# This docker file avoids using the rust provided images since we intentionally want to build for arm64/v8

FROM debian:bookworm as builder
# Install system packages
RUN apt update; apt upgrade -y
RUN apt install -y libfreetype6 libfreetype6-dev cmake build-essential curl sudo
# Configure rustup
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y --default-toolchain nightly
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup component add rust-src
RUN rustup target add wasm32-unknown-unknown
# cargo-leptos 0.2.5 has a dependency on openssl, but the git version doesn't
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall cargo-leptos -y
# RUN cargo install --locked --git https://github.com/leptos-rs/cargo-leptos cargo-leptos
RUN rustup target add aarch64-unknown-linux-gnu
RUN rustup update
# Thank you benwis https://github.com/benwis/benwis_leptos/blob/main/Dockerfile
RUN mkdir -p /app
WORKDIR /app
RUN apt install -y git pkg-config fontconfig libfontconfig1-dev binaryen
COPY . .
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN cargo leptos --manifest-path=./Cargo.toml build --release -vv

FROM debian:bookworm-slim as runner
COPY --from=builder /app/target/release/ultros /app/
COPY --from=builder /app/target/site /app/site
COPY --from=builder /app/Cargo.toml /app/
# copy font into local font dirs
COPY --from=builder /app/ultros/static/*.ttf /usr/local/share/fonts
RUN apt update; apt upgrade -y
RUN apt install libfreetype6 fontconfig -y
RUN apt-get clean
WORKDIR /app
ENV RUST_LOG="info"
ENV LEPTOS_OUTPUT_NAME="ultros"
ENV LEPTOS_ENVIRONMENT="production"
ENV LEPTOS_SITE_ADDR="0.0.0.0:8080"
ENV LEPTOS_SITE_ROOT="site"
EXPOSE 8080
CMD ["/app/ultros"]