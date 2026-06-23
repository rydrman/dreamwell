CARGO ?= cargo
CARGO_HOME ?= $(HOME)/.cargo
CARGO_BIN ?= $(CARGO_HOME)/bin
# Use the Rust wasm bundler (cargo install trunk), not trunk.io's unrelated CLI on PATH.
TRUNK ?= $(CARGO_BIN)/trunk
TRUNK_VERSION ?= v0.21.14
TRUNK_BASE_URL ?= https://github.com/trunk-rs/trunk/releases/download
TRUNK_ARCHIVE ?= trunk-x86_64-unknown-linux-gnu.tar.gz
TRUNK_URL ?= $(TRUNK_BASE_URL)/$(TRUNK_VERSION)/$(TRUNK_ARCHIVE)

KUBECONFIG ?= $(HOME)/work/homelab/kube_config_talos.yaml
IMAGE ?= ghcr.io/rydrman/dreamwell
IMAGE_TAG ?= latest
NAMESPACE ?= dreamwell

GIT_SHA := $(shell git rev-parse HEAD)
GIT_BRANCH := $(shell git rev-parse --abbrev-ref HEAD)
GIT_TAG := $(shell git describe --exact-match --tags HEAD 2>/dev/null)

COMPOSE_DEV ?= docker compose -f docker-compose.yml

.PHONY: fmt fmt-check clippy test e2e validate install-hooks install-trunk build build-front build-server run run-local run-docker clean docker deploy

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

e2e:
	./scripts/e2e-run.sh

validate: fmt-check clippy test e2e

install-hooks:
	git config core.hooksPath .githooks
	@echo "Git hooks installed from .githooks/ (pre-commit runs make validate)"

install-trunk:
	@mkdir -p $(CARGO_BIN)
	@test -x $(TRUNK) || { \
		echo "Installing trunk $(TRUNK_VERSION)..."; \
		curl -fsSL "$(TRUNK_URL)" | tar -xz -C $(CARGO_BIN) trunk; \
		chmod +x $(TRUNK); \
	}

build-front: install-trunk
	cd crates/frontend && env -u NO_COLOR $(TRUNK) build --release

build-server:
	$(CARGO) build --release -p dreamwell-server

build: build-front build-server

dev: run-docker

run-docker:
	chmod +x scripts/dev-run.sh
	@status=0; \
	$(COMPOSE_DEV) up --build --watch --exit-code-from dreamwell dreamwell \
		|| status=$$?; \
	$(COMPOSE_DEV) down --remove-orphans; \
	exit $$status

run-local: build
	DREAMWELL_STATIC_DIR=.frontend-dist \
	$(CARGO) run --release -p dreamwell-server

clean:
	$(CARGO) clean
	rm -rf .frontend-dist crates/frontend/dist

docker:
	@tags="-t $(IMAGE):$(GIT_SHA)"; \
	[ "$(GIT_BRANCH)" = main ] && tags="$$tags -t $(IMAGE):latest"; \
	[ -n "$(GIT_TAG)" ] && tags="$$tags -t $(IMAGE):$(GIT_TAG)"; \
	DOCKER_BUILDKIT=1 docker build --build-arg GIT_SHA=$(GIT_SHA) $$tags -t dreamwell:local .

deploy:
	kubectl --kubeconfig=$(KUBECONFIG) apply -k deploy/
	kubectl --kubeconfig=$(KUBECONFIG) -n $(NAMESPACE) set image deployment/dreamwell \
		dreamwell=$(IMAGE):$(IMAGE_TAG)
	kubectl --kubeconfig=$(KUBECONFIG) -n $(NAMESPACE) rollout status deployment/dreamwell
