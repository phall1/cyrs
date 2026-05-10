# Node.js FFI smoke test

Dlopens `libcyrs_ffi.{dylib,so,dll}` via [koffi], runs
`cypher_check` on a malformed query, prints diagnostics.  Spec 0004
§10.2.

[koffi]: https://koffi.dev/

## Run

```bash
# from the workspace root
cargo build -p cyrs-ffi --release

# from this directory
npm install
npm start
```

`koffi` is a single npm dep with no native build step — the package
works on any LTS Node.js with no `node-gyp` / toolchain friction.

Expected output:

```
cypher_proto_version() = 1
N diagnostic(s) for "MATCH (n RETURN n":
  [EXXXX] error L:C-L:C — ...
  ...
OK
```

## Notes

- We pick `koffi` over `ffi-napi` because ffi-napi is tied to a native
  addon that breaks on every Node.js major bump.  koffi ships pure
  WebAssembly + C for its loader and survives Node upgrades cleanly.
- The committed C header at `crates/cyrs-ffi/include/cypher.h` is the
  authoritative ABI; regenerate with `cargo xtask cbindgen`.
