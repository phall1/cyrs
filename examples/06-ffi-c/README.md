# 06-ffi-c — C program linking `libcypher_ffi`

25 lines of C. Opens a `CypherDatabase`, runs `cypher_check` on a
malformed query, walks the diagnostic list, prints each code + span +
message, frees everything.

## Build

From the repo root, build the FFI shared library once:

```sh
cargo build --release -p cypher-ffi
```

That writes `target/release/libcypher_ffi.{dylib,so,a}` and the C
header at `crates/cypher-ffi/include/cypher.h`.

From this directory:

```sh
cc -I../../crates/cypher-ffi/include \
   -L../../target/release -lcypher_ffi \
   example.c -o example
```

Or:

```sh
make
```

## Run

```sh
DYLD_LIBRARY_PATH=../../target/release ./example     # macOS
LD_LIBRARY_PATH=../../target/release  ./example      # Linux
```

Or:

```sh
make run
```

## Expected output

```
1 diagnostic(s) for "MATCH (n RETURN n":
  [E0011] error 0:9-0:9 — expected ')' to close node pattern
```

Line/column are zero-based UTF-8 (spec 0004 §5). The code `E0011` is
stable across versions.

## Memory ownership

Every `cypher_*` function that returns a pointer has a matching
`cypher_*_free`. Accessor functions (`cypher_diagnostic_code` etc.)
return borrowed pointers tied to the parent list. See
`crates/cypher-ffi/README.md` for the full contract.
