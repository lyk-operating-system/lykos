# Nuke built-in rules and variables.
MAKEFLAGS += -rR
.SUFFIXES:

# Name of the final executable will have.
OUTPUT := kernel

RUST_TARGET  ?= x86_64-unknown-none
RUST_PROFILE ?= dev

RUST_PROFILE_SUBDIR := $(RUST_PROFILE)
ifeq ($(RUST_PROFILE),dev)
	RUST_PROFILE_SUBDIR := debug
endif

.PHONY: all
all:
	RUSTFLAGS="-C relocation-model=static" cargo build \
	    --target $(RUST_TARGET) \
		--profile $(RUST_PROFILE) \
		--target-dir $(BUILD_DIR)

.PHONY: install
install:
	cp $(BUILD_DIR)/$(RUST_TARGET)/$(RUST_PROFILE_SUBDIR)/kernel $(DESTDIR)/$(OUTPUT)
