# Packages are built in an Ubuntu 24.04 container (./run-in-docker.sh) so they install on
# Ubuntu 24.04+, Debian 13+ and current Fedora/Arch. RUN= builds with the local toolchain instead.
RUN ?= ./run-in-docker.sh

.PHONY: image test deb rpm packages install

image:
	docker build -t cadence-build -f Dockerfile.build .

test:
	cargo test

deb:
	$(RUN) sh -c 'cargo build --release && cargo deb --no-build'

rpm: deb
	$(RUN) cargo generate-rpm

# .deb + .rpm + Arch package, each installed and checked in a clean container of its distro
packages: rpm
	tools/package-check.sh

install: deb
	sudo apt install ./target/debian/$$(ls -t target/debian | grep '\.deb$$' | head -1)
