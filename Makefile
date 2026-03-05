.PHONY: build test clean install uninstall run-index run-search

PREFIX ?= /usr/local

build:
	cargo build --release

test:
	cargo test

clean:
	cargo clean

install: build
	install -d $(PREFIX)/bin
	install -m 755 target/release/findex $(PREFIX)/bin/findex

uninstall:
	rm -f $(PREFIX)/bin/findex

# Development helpers
run-index:
	cargo run --release -- index $(PATH)

run-search:
	cargo run --release -- search $(TERM)
