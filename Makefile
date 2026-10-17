# RetroOS — one-command builds.
#
#   make image        flashable SD-card image  → out/retroos-<ver>-rpi-arm64.img
#   make image.xz     the same, compressed for distribution
#   make qemu         boot the QEMU variant interactively (VNC on :5901)
#   make test         unit tests + host (headless Station) tests
#   make test-qemu    Milestone end-to-end test in QEMU (slow without KVM)
#   make deb          just the retroos .deb
#   make container    build everything inside the pinned Debian build container
#   make clean
#
# Build requirements (Debian/Ubuntu host): see docs/BUILDING.md, or use `make container`.

OUT      ?= out
VERSION  := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
IMG      := $(OUT)/retroos-$(VERSION)-rpi-arm64.img
CONTAINER_IMAGE ?= retroos-build

.PHONY: all image image.xz deb qemu test test-host test-qemu container clean fmt lint

all: image

deb:
	build/mkdeb.sh $(OUT)

image:
	build/mkdeb.sh $(OUT)
	build/mkimage.sh $(OUT)

image.xz: image
	xz -T0 -6 -k -f $(IMG)
	cd $(OUT) && sha256sum $(notdir $(IMG)).xz > $(notdir $(IMG)).xz.sha256

qemu:
	qemu-system-aarch64 -M virt -cpu cortex-a76 -accel tcg,thread=multi -smp 4 -m 2048 \
	  -kernel $(OUT)/qemu/vmlinuz -initrd $(OUT)/qemu/initrd.img \
	  -append "$$(cat $(OUT)/qemu/cmdline)" \
	  -drive file=$(OUT)/qemu/root.ext4,if=virtio,format=raw,snapshot=on \
	  -device virtio-gpu-pci,xres=1280,yres=800 -device virtio-keyboard-pci -device virtio-tablet-pci \
	  -netdev user,id=n0 -device virtio-net-pci,netdev=n0,romfile= -device virtio-rng-pci \
	  -display vnc=:1 -serial mon:stdio

test: test-host
	cargo test --workspace

test-host:
	cargo build -p station -p retro-console -p retrodos -p retro-login -p retro-edit -p retro-filer -p retro-net
	python3 tests/host/station.py target/debug $(OUT)/test-host
	python3 tests/host/station.py target/debug $(OUT)/test-host-launch tests/host/launch.steps
	python3 tests/host/station.py target/debug $(OUT)/test-host-edit tests/host/edit.steps
	python3 tests/host/station.py target/debug $(OUT)/test-host-filer tests/host/filer.steps
	python3 tests/host/station.py target/debug $(OUT)/test-host-tasklist tests/host/tasklist.steps

test-qemu:
	python3 tests/qemu/test_m1.py $(OUT)
	python3 tests/qemu/test_m2.py $(OUT)

container:
	docker build -t $(CONTAINER_IMAGE) -f build/Containerfile build
	docker run --rm --privileged -v $(CURDIR):/src -w /src $(CONTAINER_IMAGE) make image OUT=$(OUT)

fmt:
	cargo fmt --all

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace -- -D warnings

clean:
	rm -rf $(OUT)
	cargo clean
