CARGO ?= cargo
TRUNK ?= trunk

.PHONY: fmt fmt-check clippy test build build-front build-server run clean docker

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

build-front:
	cd crates/frontend && env -u NO_COLOR $(TRUNK) build --release

build-server:
	$(CARGO) build --release -p dreamwell-server

build: build-front build-server

run: build
	DREAMWELL_STATIC_DIR=crates/frontend/dist \
	$(CARGO) run --release -p dreamwell-server

clean:
	$(CARGO) clean
	rm -rf crates/frontend/dist

docker:
	docker build -t dreamwell:local .
