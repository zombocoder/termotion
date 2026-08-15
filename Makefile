.PHONY: examples check fmt clippy test

examples:
	cargo build --release
	mkdir -p dist
	./target/release/termotion render examples/brb.yaml --output dist/brb.webm --overwrite
	./target/release/termotion render examples/starting-soon.yaml --output dist/starting-soon.webm --overwrite
	./target/release/termotion render examples/ending.yaml --output dist/ending.webm --overwrite

check: fmt clippy test

fmt:
	cargo fmt --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace
