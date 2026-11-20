# OEM-macOS: reproducible build entry points.
#
# Every target here runs on a clean checkout with only the documented prerequisites. A target
# that needs the network says so.

UPSTREAM ?= /workspace/vendor/reims-vgpu
CARGO ?= cargo
PREFIX ?= /usr

.PHONY: all help check test fmt clippy manifest host core install-dist clean

help:
	@echo "make check      - run every check that does not need a GPU"
	@echo "make test       - the oem-core test suite"
	@echo "make manifest   - validate every package name against the Arch repos"
	@echo "make fmt        - format the Rust code"
	@echo "make clippy     - lint the Rust code"
	@echo "make core       - build oem-core in release"
	@echo "make host       - build the minimal host package list (needs Arch)"
	@echo "make install-dist DESTDIR=... - install the OEM files"

all: check

# The single command CI runs. Everything here must pass on a machine with no GPU and no KVM.
check: fmt clippy test manifest
	@echo "all checks passed"

test:
	cd oem && $(CARGO) test --all-targets

fmt:
	cd oem && $(CARGO) fmt --check

clippy:
	cd oem && $(CARGO) clippy --all-targets -- -D warnings

manifest:
	./tools/check-manifest.py host/packages/packages.txt

core:
	cd oem && $(CARGO) build --release
	@echo "built oem/target/release/oem-core"

# The host's package list, for a machine that is already Arch.
host: manifest
	@echo "Installing the host packages. This changes the running system."
	sudo pacman -S --needed --noconfirm $$(sed 's/#.*//' host/packages/packages.txt | tr -d '[:space:]' | grep -v '^$$')

# Install the OEM files into an already-prepared root.
install-dist:
	install -Dm755 oem/target/release/oem-core $(DESTDIR)$(PREFIX)/bin/oem-core
	install -Dm755 host/installer/oem-install $(DESTDIR)$(PREFIX)/bin/oem-install
	install -Dm644 host/systemd/oem-core.service $(DESTDIR)$(PREFIX)/lib/systemd/system/oem-core.service
	install -Dm644 host/systemd/oem-maintenance.service $(DESTDIR)$(PREFIX)/lib/systemd/system/oem-maintenance.service
	install -Dm644 host/systemd/oem-poweroff-on-guest-stop.path $(DESTDIR)$(PREFIX)/lib/systemd/system/oem-poweroff-on-guest-stop.path
	install -Dm644 host/systemd/oem-poweroff-on-guest-stop.service $(DESTDIR)$(PREFIX)/lib/systemd/system/oem-poweroff-on-guest-stop.service
	install -Dm644 host/systemd/seatd.service $(DESTDIR)$(PREFIX)/lib/systemd/system/seatd.service
	install -Dm644 host/kernel/oem-sysctl.conf $(DESTDIR)/etc/sysctl.d/90-oem.conf
	install -Dm644 host/kernel/oem-modules-load.conf $(DESTDIR)/etc/modules-load.d/oem.conf
	install -Dm644 host/mkinitcpio/oem-macos.conf $(DESTDIR)/etc/mkinitcpio.conf
	install -Dm644 host/mkinitcpio/oem-macos.preset $(DESTDIR)/etc/mkinitcpio.d/oem-macos.preset
	install -d $(DESTDIR)$(PREFIX)/share/doc/oem-macos
	install -m644 docs/*.md $(DESTDIR)$(PREFIX)/share/doc/oem-macos/

clean:
	cd oem && $(CARGO) clean
