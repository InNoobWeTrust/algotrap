.DEFAULT_GOAL := help

.PHONY: help trigger-nightly-workflow duckdb-test \
	ci-setup-qemu ci-setup-builder \
	ci-verify-cryptobot ci-verify-telegrambot ci-verify-images

## General commands

help: ## Display available commands.
	@grep -E '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) | \
		awk 'BEGIN { FS = ":.*?## " } { printf "%-28s %s\n", $$1, $$2 }'

trigger-nightly-workflow: ## Trigger the GitHub nightly workflow.
	gh workflow run nightly.yml

duckdb-test: ## Test the workspace with the configured DuckDB client.
	cargo test --workspace --locked

## Local multi-platform container CI

# All `ci-verify-*` targets build final Dockerfile stages without pushing images.
# Each image is built for every configured platform in one Buildx invocation.
BUILDER ?= algotrap-ci
PLATFORMS ?= linux/amd64,linux/arm64
QEMU_IMAGE ?= tonistiigi/binfmt:latest

ci-setup-qemu: ## Register QEMU emulators for multi-platform builds.
	docker run --privileged --rm "$(QEMU_IMAGE)" --install all

ci-setup-builder: ## Create and validate the containerized Buildx builder.
	@if ! docker buildx inspect "$(BUILDER)" >/dev/null 2>&1; then \
		docker buildx create --name "$(BUILDER)" --driver docker-container; \
	fi
	@if ! docker buildx inspect "$(BUILDER)" --bootstrap | \
		grep -Eq '^Driver:[[:space:]]+docker-container$$'; then \
		echo "Buildx builder '$(BUILDER)' must use docker-container" >&2; \
		exit 2; \
	fi

ci-verify-cryptobot: ci-setup-qemu ci-setup-builder ## Verify cryptobot images for all platforms.
	docker buildx build \
		--builder "$(BUILDER)" \
		--platform "$(PLATFORMS)" \
		--file bins/cryptobot/deployment/Dockerfile \
		--progress=plain \
		--provenance=false \
		--output=type=cacheonly \
		.

ci-verify-telegrambot: ci-setup-qemu ci-setup-builder ## Verify telegrambot images for all platforms.
	docker buildx build \
		--builder "$(BUILDER)" \
		--platform "$(PLATFORMS)" \
		--file bins/telegrambot/deployment/Dockerfile \
		--progress=plain \
		--provenance=false \
		--output=type=cacheonly \
		.

ci-verify-images: ci-verify-cryptobot ci-verify-telegrambot ## Verify all production container images.
