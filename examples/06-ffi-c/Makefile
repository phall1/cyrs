# Minimal build: link against libcypher_ffi.dylib / .so from target/release.
# Run `cargo build --release -p cypher-ffi` first.

CC     ?= cc
CFLAGS ?= -Wall -Wextra -O2
ROOT   := ../..

example: example.c
	$(CC) $(CFLAGS) -I$(ROOT)/crates/cypher-ffi/include \
	    -L$(ROOT)/target/release -lcypher_ffi \
	    example.c -o example

run: example
	DYLD_LIBRARY_PATH=$(ROOT)/target/release \
	LD_LIBRARY_PATH=$(ROOT)/target/release \
	./example

clean:
	rm -f example

.PHONY: run clean
