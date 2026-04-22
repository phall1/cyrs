/* Minimal C consumer of libcypher_ffi. Runs `cypher_check` on a
 * malformed query and walks the diagnostic list.
 *
 * Build / run — see README.md.
 */

#include <stdio.h>
#include <string.h>
#include "cypher.h"

int main(void) {
    if (cypher_proto_version() != 1) {
        fprintf(stderr, "unexpected proto version\n");
        return 2;
    }

    CypherDatabase *db = cypher_database_new();
    if (!db) {
        fprintf(stderr, "cypher_database_new: %s\n", cypher_last_error());
        return 2;
    }

    const char *src = "MATCH (n RETURN n";
    CypherDiagnosticList *list = cypher_check(db, src, strlen(src));
    if (!list) {
        fprintf(stderr, "cypher_check: %s\n", cypher_last_error());
        cypher_database_free(db);
        return 2;
    }

    size_t n = cypher_diagnostic_list_len(list);
    printf("%zu diagnostic(s) for \"%s\":\n", n, src);
    for (size_t i = 0; i < n; i++) {
        printf("  [%s] %s %u:%u-%u:%u — %s\n",
               cypher_diagnostic_code(list, i),
               cypher_diagnostic_severity(list, i),
               cypher_diagnostic_start_line(list, i),
               cypher_diagnostic_start_col(list, i),
               cypher_diagnostic_end_line(list, i),
               cypher_diagnostic_end_col(list, i),
               cypher_diagnostic_message(list, i));
    }

    cypher_diagnostic_list_free(list);
    cypher_database_free(db);
    return n > 0 ? 0 : 1;
}
