.DEFAULT_GOAL := main

.PHONY: .cargo
.cargo: ## Check that cargo is installed
	@cargo --version || echo 'Please install cargo: https://github.com/rust-lang/cargo'

.PHONY: .pre-commit
.pre-commit: ## Check that pre-commit is installed
	@pre-commit -V || echo 'Please install pre-commit: https://pre-commit.com/'

.PHONY: install-js
install-js: ## Install JS package dependencies
	cd crates/monty-js && npm install

.PHONY: install
install: .cargo .pre-commit install-js ## Install dependencies and pre-commit for local development
	cargo check --workspace
	pre-commit install --install-hooks

.PHONY: dev-js
dev-js: ## Build the JS package (debug)
	cd crates/monty-js && npm run build:debug

.PHONY: lint-js
lint-js: install-js ## Lint JS code with oxlint
	cd crates/monty-js && npm run lint

.PHONY: test-js
test-js: dev-js ## Build and test the JS package
	cd crates/monty-js && npm test

.PHONY: smoke-test-js
smoke-test-js: ## Run smoke test for JS package (builds, packs, and tests installation)
	cd crates/monty-js && npm run smoke-test

.PHONY: dev-js-release
dev-js-release: ## Build the JS package (release)
	cd crates/monty-js && npm run build

.PHONY: format-rs
format-rs:  ## Format Rust code with fmt
	@cargo +nightly fmt --version
	cargo +nightly fmt --all

.PHONY: format-js
format-js: install-js ## Format JS code with prettier
	cd crates/monty-js && npm run format:prettier

.PHONY: format
format: format-rs format-js ## Format Rust and JS code

.PHONY: lint-rs
lint-rs:  ## Lint Rust code with clippy and import checks
	@cargo clippy --version
	cargo clippy --workspace --tests --bench main -- -D warnings
	cargo clippy --workspace --tests --all-features -- -D warnings
	cargo test -p monty --test import_order

.PHONY: clippy-fix
clippy-fix: ## Fix Rust code with clippy
	cargo clippy --workspace --tests --bench main --all-features --fix --allow-dirty

.PHONY: lint
lint: lint-rs lint-js ## Lint Rust and JS code

.PHONY: format-lint-rs
format-lint-rs: format-rs lint-rs ## Format and lint Rust code with fmt and clippy

.PHONY: test-no-features
test-no-features: ## Run rust tests without any features enabled
	cargo test -p monty

.PHONY: test-ref-count-panic
test-ref-count-panic: ## Run rust tests with ref-count-panic enabled
	cargo test -p monty --features ref-count-panic

.PHONY: test-ref-count-return
test-ref-count-return: ## Run rust tests with ref-count-return enabled
	cargo test -p monty --features ref-count-return

.PHONY: test-cases
test-cases: ## Run tests cases only
	cargo test -p monty --test datatest_runner

.PHONY: test-type-checking
test-type-checking: ## Run rust tests on monty_type_checking
	cargo test -p monty_type_checking -p monty_typeshed

.PHONY: test-docs
test-docs: ## Test Rust docs examples only
	cargo test --doc -p monty

.PHONY: test
test: test-ref-count-panic test-ref-count-return test-no-features test-type-checking ## Run Rust tests

.PHONY: testcov
testcov: ## Run Rust tests with coverage, print table, and generate HTML report
	@cargo llvm-cov --version > /dev/null 2>&1 || echo 'Please run: `cargo install cargo-llvm-cov`'
	cargo llvm-cov clean --workspace
	echo "coverage for `make test-no-features`"
	cargo llvm-cov --no-report -p monty
	echo "coverage for `make test-ref-count-panic`"
	cargo llvm-cov --no-report -p monty --features ref-count-panic
	echo "coverage for `make test-ref-count-return`"
	cargo llvm-cov --no-report -p monty --features ref-count-return
	echo "coverage for `make test-type-checking`"
	cargo llvm-cov --no-report -p monty_type_checking -p monty_typeshed
	echo "Generating reports:"
	cargo llvm-cov report --ignore-filename-regex '(tests/|test_cases/|/tests\.rs$$)'
	cargo llvm-cov report --html --ignore-filename-regex '(tests/|test_cases/|/tests\.rs$$)'
	@echo ""
	@echo "HTML report: $${CARGO_TARGET_DIR:-target}/llvm-cov/html/index.html"

.PHONY: bench
bench: ## Run benchmarks
	cargo bench -p monty --bench main

.PHONY: dev-bench
dev-bench: ## Run benchmarks to test with dev profile
	cargo bench --profile dev -p monty --bench main -- --test

.PHONY: profile
profile: ## Profile the code with pprof and generate flamegraphs
	cargo bench -p monty --bench main --profile profiling -- --profile-time=10

.PHONY: type-sizes
type-sizes: ## Write type sizes for the crate to ./type-sizes.txt (requires nightly and top-type-sizes)
	RUSTFLAGS="-Zprint-type-sizes" cargo +nightly build -j1 2>&1 | top-type-sizes -f '^monty.*' > type-sizes.txt
	@echo "Type sizes written to ./type-sizes.txt"

.PHONY: fuzz-string_input_panic
fuzz-string_input_panic: ## Run the `string_input_panic` fuzz target
	cargo +nightly fuzz run --fuzz-dir crates/fuzz string_input_panic

.PHONY: fuzz-tokens_input_panic
fuzz-tokens_input_panic: ## Run the `tokens_input_panic` fuzz target (structured token input)
	cargo +nightly fuzz run --fuzz-dir crates/fuzz tokens_input_panic

.PHONY: main
main: lint test-ref-count-panic test-no-features ## run linting and the most important tests

# (must stay last!)
.PHONY: help
help: ## Show this help (usage: make help)
	@echo "Usage: make [recipe]"
	@echo "Recipes:"
	@awk '/^[a-zA-Z0-9_-]+:.*?##/ { \
	    helpMessage = match($$0, /## (.*)/); \
	        if (helpMessage) { \
	            recipe = $$1; \
	            sub(/:/, "", recipe); \
	            printf "  \033[36mmake %-20s\033[0m %s\n", recipe, substr($$0, RSTART + 3, RLENGTH); \
	    } \
	}' $(MAKEFILE_LIST)
