// Go smoke test for the cyrs-ffi C ABI.  Spec 0004 §10.2.
//
// Build:
//
//	cargo build -p cyrs-ffi --release
//	cd demo/ffi/go
//	CGO_LDFLAGS="-L../../../target/release -lcypher_ffi" go run main.go
//
// The program dlopens the produced `libcyrs_ffi.{dylib,so}`, runs
// `cypher_check` on a malformed query (`MATCH (n RETURN n` — unclosed
// paren), and prints every diagnostic to stdout.  Exit code 0 iff at
// least one diagnostic was reported.

package main

/*
#cgo CFLAGS: -I../../../crates/cyrs-ffi/include
#include <stdlib.h>
#include <string.h>
#include "cypher.h"
*/
import "C"

import (
	"fmt"
	"os"
	"unsafe"
)

func main() {
	proto := C.cypher_proto_version()
	if proto != 1 {
		fmt.Fprintf(os.Stderr, "unexpected proto_version: %d (want 1)\n", proto)
		os.Exit(2)
	}
	fmt.Printf("cypher_proto_version() = %d\n", proto)

	db := C.cypher_database_new()
	if db == nil {
		fmt.Fprintln(os.Stderr, "cypher_database_new returned NULL")
		os.Exit(2)
	}
	defer C.cypher_database_free(db)

	src := "MATCH (n RETURN n"
	cSrc := C.CString(src)
	defer C.free(unsafe.Pointer(cSrc))

	list := C.cypher_check(db, cSrc, C.size_t(len(src)))
	if list == nil {
		// Fall back to cypher_last_error for context.
		errPtr := C.cypher_last_error()
		err := C.GoString(errPtr)
		fmt.Fprintf(os.Stderr, "cypher_check returned NULL: %s\n", err)
		os.Exit(2)
	}
	defer C.cypher_diagnostic_list_free(list)

	n := C.cypher_diagnostic_list_len(list)
	fmt.Printf("%d diagnostic(s) for %q:\n", n, src)
	for i := C.size_t(0); i < n; i++ {
		code := C.GoString(C.cypher_diagnostic_code(list, i))
		severity := C.GoString(C.cypher_diagnostic_severity(list, i))
		msg := C.GoString(C.cypher_diagnostic_message(list, i))
		sl := C.cypher_diagnostic_start_line(list, i)
		sc := C.cypher_diagnostic_start_col(list, i)
		el := C.cypher_diagnostic_end_line(list, i)
		ec := C.cypher_diagnostic_end_col(list, i)
		fmt.Printf("  [%s] %s %d:%d-%d:%d — %s\n", code, severity, sl, sc, el, ec, msg)
	}

	if n < 1 {
		fmt.Fprintln(os.Stderr, "expected at least one diagnostic; got zero")
		os.Exit(1)
	}
	fmt.Println("OK")
}
