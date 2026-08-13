.DEFAULT_GOAL := test

.PHONY: check fmt format-rs lint lint-rs test build

check:
	cargo check --workspace --locked

fmt format-rs:
	cargo fmt --all

lint lint-rs:
	cargo clippy --workspace --all-targets --locked -- -D warnings

test:
	cargo test --workspace --locked

build:
	cargo build --workspace --locked
