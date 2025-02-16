#############################################
# Makefile for Cargo and Python Project
#
# Targets:
#   all           : Build release binaries (default target).
#   docs          : Generate documentation by analyzing Cargo modules and rendering GraphViz graphs.
#   build         : Build the Cargo project in debug mode.
#   release       : Build the Cargo project in release mode and list the resulting binaries.
#   lint          : Run Rust linter (cargo clippy) and check Python formatting (black) on utils and tests.
#   test          : Run both Cargo tests and Python unit tests.
#   test_cargo    : Run Cargo tests.
#   test_python   : Run Python unit tests (Data Cruncher and Connection Stress Test).
#   clean         : Clean build artifacts using Cargo's built-in clean command.
#   run_server    : Run the 'portredirect_server' binary with extra arguments. Pass args via the ARGS variable.
#   run_client    : Run the 'portredirect_client' binary with extra arguments. Pass args via the ARGS variable.
#
# Usage Examples:
#   make
#       Builds release binaries (default).
#
#   make build
#       Builds the project in debug mode.
#
#   make release
#       Builds the project in release mode.
#
#   make lint
#       Runs lint checks on Rust and Python code.
#
#   make test
#       Runs all tests (Cargo and Python).
#
#   make clean
#       Cleans build artifacts.
#
#   make run_server ARGS="--arg1 value1 --arg2 value2"
#       Runs the server binary with additional arguments.
#
#   make run_client ARGS="--argA valueA"
#       Runs the client binary with additional arguments.
#############################################

.PHONY: all docs build release lint test test_cargo test_python clean run_server run_client

# Default target: build for release.
all: test release

# ------------------------------
# Documentation targets.
# ------------------------------
docs:
	@echo "Analyzing Cargo module..."
	@./utils/docs-generate-cargo-modules-tree.sh
	@echo "Render GraphViz graphs for Docs."
	@./utils/render-diagrams.sh ./docs/*.dot

# ------------------------------
# Build targets.
# ------------------------------
# Build the Cargo project (debug mode).
build:
	@echo "Building Cargo project (debug mode)..."
	@cargo build

# Build the Cargo project in release mode.
release:
	@echo "Building Cargo project (release mode)..."
	@cargo build --release
	@ls -lh target/release

# ------------------------------
# Lint targets.
# ------------------------------
# Run Rust linter and Python code formatter checks.
lint:
	@echo "Running Rust linter (cargo clippy)..."
	@cargo clippy --all-targets --all-features -- -D warnings
	@echo "Running Python code formatter check (black) on utils and tests..."
	@if command -v black >/dev/null 2>&1; then \
		black --check utils/*.py tests/*.py; \
	else \
		echo "Black is not installed. Skipping Python formatting check."; \
	fi

# Automatically fix lint issues.
lint_fix:
	@echo "Running cargo fix to automatically apply Rust suggestions..."
	@cargo fix --allow-dirty --allow-staged
	@echo "Running black to auto-format Python files in utils and tests..."
	@if command -v black >/dev/null 2>&1; then \
		black utils/*.py tests/*.py; \
	else \
		echo "Black is not installed. Skipping Python formatting fix."; \
	fi
	
# ------------------------------
# Test targets.
# ------------------------------
# Top-level test target: runs both Cargo and Python tests.
test: test_cargo test_python

# Run Cargo tests.
test_cargo:
	@echo "Running Cargo tests..."
	@cargo test

# Run Python unit tests.
test_python:
	@echo "Running Python unit tests (Data Cruncher)..."
	@cd utils && python -m unittest test_cst_datacruncher.py
	@echo "Running Python unit tests (Connection Stress Test)..."
	@cd utils && python -m unittest test_connection_stress_test.py

# ------------------------------
# Clean target.
# ------------------------------
# Clean build artifacts using Cargo's built-in clean command.
clean:
	@echo "Cleaning build artifacts..."
	@cargo clean

# ------------------------------
# Run targets.
# ------------------------------
# Run the portredirect_server binary. Pass extra arguments via ARGS.
run_server:
	@echo "Running portredirect_server..."
	@cargo run --bin portredirect_server -- $(ARGS)

# Run the portredirect_client binary. Pass extra arguments via ARGS.
run_client:
	@echo "Running portredirect_client..."
	@cargo run --bin portredirect_client -- $(ARGS)
