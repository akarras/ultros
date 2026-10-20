# Loro block decoding budget

Source: crates.io `loro-kv-store` 1.16.0, matching the existing pinned Loro 1.16.2 dependency. The original MIT license and upstream tests are retained.

Ultros adds an opt-in thread-local cumulative budget for decompressed block bytes. The LZ4 reader copies bounded chunks and checks the remaining budget before extending the output vector. Uncompressed blocks consume the same budget. A nested operation cannot increase its caller's budget; the guard restores thread state on normal exit and unwinding.

Budgeted SSTable imports eagerly validate every block through the fallible import path before installing the table. Re-decoding already validated blocks after cache eviction does not spend that budget again; each block still has a 16 MiB hard expansion limit. This ordering matters because Loro's lazy getters assume validation succeeded and panic on a decode error; rejecting there can poison an internal mutex and cause a second panic during destruction. The compressed obsolete-history integration regression verifies an ordinary error instead of a process abort. The upstream workspace-only test logger call is omitted from the standalone vendored library tests; test assertions are unchanged.

This addresses compressed-history expansion before projection validation. It does not promise containment of every decoder allocation, stack overflow, OOM, or native fault. Callers still bound encoded input, document structure/history, concurrency, and persisted output. The wire format and encoder are unchanged.

Run the `compress::budget_tests` test and the upstream library/integration tests when changing this patch. The Lists 2.0 convergence and compatibility suites remain required.
