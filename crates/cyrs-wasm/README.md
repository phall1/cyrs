# cyrs-wasm

[![crates.io](https://img.shields.io/crates/v/cyrs-wasm.svg)](https://crates.io/crates/cyrs-wasm)
[![docs.rs](https://img.shields.io/docsrs/cyrs-wasm)](https://docs.rs/cyrs-wasm)
[![CI](https://github.com/phall1/cyrs/actions/workflows/ci.yml/badge.svg)](https://github.com/phall1/cyrs/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.94-blue)](https://github.com/phall1/cyrs/blob/main/rust-toolchain.toml)
[![License: Apache-2.0 OR MIT](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

WebAssembly binding for the Cyrs Cypher / GQL frontend. Exposes the
agent v1 op surface (parse, check, complete, hover, format, rewrite,
plan, explain, schema_set, schema_clear) as JavaScript callables on a
single `CypherDatabase` class. Thin adapter over `cyrs-lang-services`
+ `cyrs-db` + `cyrs-diag` + `cyrs-fmt`; no analysis logic lives
here (spec 0004 §4).

## Build for the browser

```bash
# one-off — install the wasm32 target
rustup target add wasm32-unknown-unknown

# build the cdylib
cargo build -p cyrs-wasm --target wasm32-unknown-unknown --release

# generate the JS wrapper (cargo install wasm-bindgen-cli --locked)
wasm-bindgen --target web \
    --out-dir ./pkg \
    target/wasm32-unknown-unknown/release/cyrs_wasm.wasm

# (optional) optimise — wasm-opt ships with binaryen
wasm-opt -Os -o ./pkg/cyrs_wasm_bg.wasm ./pkg/cyrs_wasm_bg.wasm

# brotli-compress for the size gate (spec 0004 §4.2, ≤ 2 MB)
brotli -q 11 ./pkg/cyrs_wasm_bg.wasm -o ./pkg/cyrs_wasm_bg.wasm.br
```

`cargo xtask wasm-size` drives that full pipeline end-to-end and fails
if the brotli-compressed artifact exceeds the 2 MB size budget. The
pipeline depends on `wasm-bindgen-cli`, `wasm-opt` (binaryen), and
`brotli`; missing tools are reported as a skip rather than a failure
(CI installs them, local developers can skip).

## JS API shape

```js
import init, { CypherDatabase } from "./pkg/cyrs_wasm.js";

await init();

// protoVersion() is a static. Reject dispatch if it diverges from the
// wire version your code was built against (spec 0004 §4.3).
if (CypherDatabase.protoVersion() !== 1) throw new Error("proto mismatch");

const db = new CypherDatabase();
const result = db.check("MATCH (n) RETURN n");
console.log(result.diagnostics);  // [] for well-formed input
```

Every method mirrors the agent op of the same name; see the spec 0001
§15 wire table for the full input / output shape.

## Size budget

Spec 0004 §4.2 caps the brotli-compressed `.wasm` at **2 MB**. Nightly
CI (`nightly-benches.yml` + `cargo xtask wasm-size`) regenerates the
artifact end-to-end and fails the build on breach.

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](https://github.com/phall1/cyrs/blob/main/LICENSE-APACHE))
- MIT license
  ([LICENSE-MIT](https://github.com/phall1/cyrs/blob/main/LICENSE-MIT))

at your option.
