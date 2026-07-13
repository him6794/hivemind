.PHONY: help dev build test clean proto docker-build docker-up docker-down

# Default target
help:
	@echo "Hivemind Rust Build System"
	@echo ""
	@echo "Commands:"
	@echo "  make dev              - Start development environment"
	@echo "  make build            - Build Rust binary"
	@echo "  make test             - Run all tests"
	@echo "  make clean            - Clean build artifacts"
	@echo "  make proto            - Generate protobuf code"
	@echo "  make docker-build     - Build Docker images"
	@echo "  make docker-up        - Start Docker Compose"
	@echo "  make docker-down      - Stop Docker Compose"
	@echo "  make lint             - Run linter"
	@echo "  make fmt              - Format code"
	@echo "  make release VERSION=1.0.0 MONTY=path\\to\\monty.exe"

# ============================================
# Development
# ============================================

dev: build
	@echo "Starting development environment..."
	@docker-compose up -d redis postgres
	@echo "Waiting for databases..."
	@sleep 5
	@echo "Starting Hivemind..."
	@cd hivemind-rs && cargo run --bin hivemind-bin -- all
	@echo "Development environment started"
	@echo "API: http://localhost:8082"
	@echo "gRPC: localhost:50051"

# ============================================
# Build
# ============================================

build:
	@echo "Building Hivemind..."
	@cd hivemind-rs && cargo build --release
	@echo "Build complete"

build-debug:
	@echo "Building Hivemind (debug)..."
	@cd hivemind-rs && cargo build
	@echo "Build complete"

build-frontend:
	@echo "Building frontend..."
	@cd frontend/master-ui && npm run build
	@cd frontend/worker-ui && npm run build
	@echo "Frontend build complete"

release:
	@if [ -z "$(VERSION)" ] || [ -z "$(MONTY)" ]; then echo "Usage: make release VERSION=1.0.0 MONTY=path/to/monty"; exit 1; fi
	@powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release.ps1 -Version "$(VERSION)" -MontyExecutable "$(MONTY)"

# ============================================
# Test
# ============================================

test:
	@echo "Running tests..."
	@cd hivemind-rs && cargo test
	@echo "Tests complete"

test-verbose:
	@echo "Running tests (verbose)..."
	@cd hivemind-rs && cargo test -- --nocapture
	@echo "Tests complete"

# ============================================
# Protobuf
# ============================================

proto:
	@echo "Generating protobuf code..."
	@cd hivemind-rs && cargo build -p hivemind-proto
	@echo "Protobuf generation complete"

# ============================================
# Docker
# ============================================

docker-build:
	@echo "Building Docker images..."
	@docker-compose build
	@echo "Docker build complete"

docker-up:
	@echo "Starting Docker Compose..."
	@docker-compose up -d
	@echo "Services started"
	@docker-compose ps

docker-down:
	@echo "Stopping Docker Compose..."
	@docker-compose down
	@echo "Services stopped"

docker-logs:
	@docker-compose logs -f

docker-clean:
	@echo "Cleaning Docker data..."
	@docker-compose down -v
	@docker system prune -f
	@echo "Docker cleanup complete"

# ============================================
# Lint & Format
# ============================================

lint:
	@echo "Running linter..."
	@cd hivemind-rs && cargo clippy -- -D warnings
	@echo "Lint complete"

fmt:
	@echo "Formatting code..."
	@cd hivemind-rs && cargo fmt
	@echo "Format complete"

# ============================================
# Database
# ============================================

db-reset:
	@echo "Resetting database..."
	@docker-compose down postgres redis
	@docker volume rm hivemind-postgres-data hivemind-redis-data || true
	@docker-compose up -d postgres redis
	@echo "Database reset complete"

# ============================================
# Clean
# ============================================

clean:
	@echo "Cleaning build artifacts..."
	@cd hivemind-rs && cargo clean
	@rm -rf frontend/master-ui/dist
	@rm -rf frontend/worker-ui/dist
	@echo "Clean complete"

clean-all: clean docker-clean
	@echo "Full cleanup complete"
