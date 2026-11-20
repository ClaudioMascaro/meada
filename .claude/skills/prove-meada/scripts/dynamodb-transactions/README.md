# Neutral DynamoDB transaction entries

This independent source key covers #772 and #774 through #894. The Java
transaction wrapper reads List.of, Arrays.asList, one entry or a list of
read entries. Sorting changes order, and an edit after the wrapper returns
does not change its request. A second unchanged wrapper forwards the list.
The deliberately opaque forms leave the table unnamed. The Go entry twins
keep the same SDK request shape and change which local a closure writes.

| Source | Site | Role | Table |
|---|---|---|---|
| Transactions.java | immutableList | writer | immutable |
| Transactions.java | arrayListView | writer | array-view |
| Transactions.java | oneEntry | writer | one-entry |
| Transactions.java | getList | reader | lookup |
| Transactions.java | sortedEntries | writer | sorted-cedar |
| Transactions.java | sortedEntries | writer | sorted-elm |
| Transactions.java | editAfterCall | writer | before-edit |
| Transactions.java | twiceWrapped | writer | sends-through-bridge |
| Transactions.java | mutatedBeforeRead | writer | unresolved |
| Transactions.java | sameValueTwice | writer | unresolved |
| Transactions.java | writeVarargs | writer | unresolved |
| Transactions.java | writeVarargs | reader | unresolved |
| Transactions.java | withAlias | writer | unresolved |
| Transactions.java | deferredRead | writer | unresolved |
| Transactions.java | overridden | writer | unresolved |
| entries.go | closureWrite | writer | unresolved |
| entries.go | closureControl | writer | closure-control |
| entries.go | goroutineWrite | writer | unresolved |
| entries.go | goroutineControl | writer | goroutine-control |
| entries.go | deferredWrite | writer | unresolved |
| entries.go | deferredControl | writer | defer-control |

The Go deferred write is after the call at runtime, but its target is a
shared local with a further mention. #774's conservative named-entry
guard leaves it unnamed. In the control the write touches only another
local. These cases have no cloud credentials, live service or network.
The variadic declaration itself has a resource-typed argument without a
table, separate from the unresolved writer at the SDK call.

All source is indexed alone via the public CLI. MCP search returns the source
file and contract for each named table. Context on `TransactionExamples.immutableList()`
shows a writer of `immutable`, and `TransactionExamples.getList()` shows a reader
of `lookup`. Context on `deferredWrite` shows the unnamed DynamoDB writer, and
`deferredControl` shows the writer of `defer-control`. Comparing the base on
this new flow is a separate maintainer decision in #894.
