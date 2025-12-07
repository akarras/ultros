small-wasm:
	cargo build --release --target wasm32-unknown-unknown -Z build-std=panic_abort,std --target-dir ./target/optimized-wasm -p ultros-client

