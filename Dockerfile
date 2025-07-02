# This docker file avoids using the rust provided images since we intentionally want to build for arm64/v8

FROM debian:bookworm as builder
# Install system packages
RUN apt update; apt upgrade -y
RUN apt install -y libfreetype6 libfreetype6-dev cmake build-essential curl sudo pkg-config libssl-dev
# Configure rustup
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y --default-toolchain nightly
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup component add rust-src
RUN rustup target add wasm32-unknown-unknown
# cargo-leptos 0.2.5 has a dependency on openssl, but the git version doesn't
RUN cargo install --locked cargo-leptos --version 0.2.36
RUN rustup update
RUN mkdir -p /app
WORKDIR /app
RUN apt install -y git pkg-config fontconfig libfontconfig1-dev
COPY . .
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
# ENV WASM_BINDGEN_WEAKREF=1
RUN cargo leptos --manifest-path=./Cargo.toml build --release -vv

FROM debian:bookworm-slim as runner
COPY --from=builder /app/target/release/ultros /app/
COPY --from=builder /app/target/site /app/site
COPY --from=builder /app/Cargo.toml /app/
# copy font into local font dirs
RUN mkdir /usr/local/share/fonts
COPY --from=builder /app/ultros/static/*.ttf /usr/local/share/fonts
RUN apt update; apt upgrade -y
RUN apt install libfreetype6 fontconfig libfontconfig1 -y; apt-get clean; rm -rf /var/lib/apt/lists/*;
WORKDIR /app
ENV RUST_LOG="info"
ENV LEPTOS_OUTPUT_NAME="ultros"
ENV LEPTOS_ENVIRONMENT="production"
ENV LEPTOS_SITE_ADDR="0.0.0.0:8080"
ENV LEPTOS_SITE_ROOT="site"
EXPOSE 8080
CMD ["/app/ultros"]
