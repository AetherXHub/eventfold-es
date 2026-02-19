# eventfold-es -- unified check / format / lint / test targets
#
# Usage:
#   make check      Run all checks (formatting, linting, tests)
#   make fmt        Auto-fix formatting
#   make lint       Run linters (clippy)
#   make test       Run all tests

.PHONY: check fmt fmt-check lint test

# ---------------------------------------------------------------------------
# Aggregate targets
# ---------------------------------------------------------------------------

check: fmt-check lint test
	@echo "All checks passed."

# ---------------------------------------------------------------------------
# Formatting
# ---------------------------------------------------------------------------

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

# ---------------------------------------------------------------------------
# Linting
# ---------------------------------------------------------------------------

lint:
	cargo clippy -- -D warnings

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

test:
	cargo test
