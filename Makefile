# eventfold-es -- unified check / format / lint / test targets
#
# Usage:
#   make check      Run all checks (formatting, linting, types, tests)
#   make fmt        Auto-fix formatting (Rust + TypeScript/React)
#   make lint       Run linters (clippy + ESLint)
#   make test       Run all tests

.PHONY: check fmt fmt-check lint test build

CRM_DIR := examples/crm

# ---------------------------------------------------------------------------
# Aggregate targets
# ---------------------------------------------------------------------------

check: fmt-check lint build test
	@echo "All checks passed."

# ---------------------------------------------------------------------------
# Formatting
# ---------------------------------------------------------------------------

fmt:
	cargo fmt
	cd $(CRM_DIR) && npx prettier --write 'src-frontend/src/**/*.{ts,tsx,css}'

fmt-check:
	cargo fmt --check
	cd $(CRM_DIR) && npx prettier --check 'src-frontend/src/**/*.{ts,tsx,css}'

# ---------------------------------------------------------------------------
# Linting + type checking
# ---------------------------------------------------------------------------

lint:
	cargo clippy -- -D warnings
	cd $(CRM_DIR) && npx tsc --noEmit
	cd $(CRM_DIR) && npx eslint src-frontend/src/

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

test:
	cargo test

# ---------------------------------------------------------------------------
# Frontend build (Vite production build)
# ---------------------------------------------------------------------------

build:
	cd $(CRM_DIR) && npm run build
