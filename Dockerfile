FROM rustlang/rust:nightly-bullseye as builder

# Thank you benwis https://github.com/benwis/benwis_leptos/blob/main/Dockerfile
RUN cargo install --locked cargo-leptos
RUN rustup component add rust-src --toolchain nightly-x86_64-unknown-linux-gnu
RUN rustup target add wasm32-unknown-unknown
RUN mkdir -p /app
WORKDIR /app
COPY . .
ENV LEPTOS_BIN_TARGET_TRIPLE="x86_64-unknown-linux-gnu"
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN cargo leptos --manifest-path=./Cargo.toml build --release -vv

FROM rustlang/rust:nightly-bullseye as runner
COPY --from=builder /app/target/server/release/ultros /app/
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