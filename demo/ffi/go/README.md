# Go FFI smoke test

Dlopens `libcypher_ffi.{dylib,so}` via cgo, runs `cypher_check` on a
malformed query, prints diagnostics.  Spec 0004 §10.2.

## Run

```bash
# from the workspace root
cargo build -p cypher-ffi --release

# from this directory
CGO_LDFLAGS="-L../../../target/release -lcypher_ffi" go run main.go
```

Expected output:

```
cypher_proto_version() = 1
N diagnostic(s) for "MATCH (n RETURN n":
  [EXXXX] error L:C-L:C — ...
  ...
OK
```

Exit code 0 on success; non-zero if zero diagnostics surface or a call
returns `NULL`.

## Notes

- On Linux add `-Wl,-rpath,../../../target/release` to `CGO_LDFLAGS` so
  the dynamic loader can find the library at runtime.
- The committed header at `crates/cypher-ffi/include/cypher.h` is the
  authoritative interface; regenerate with `cargo xtask cbindgen`.
