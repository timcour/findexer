.PHONY: build test clean run-index run-search

build:
	cargo build --release

test:
	cargo test

clean:
	cargo clean

# Development helpers
run-index:
	cargo run --release -- index $(PATH)

run-search:
	cargo run --release -- search $(TERM)
