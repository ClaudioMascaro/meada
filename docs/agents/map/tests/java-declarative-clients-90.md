# Java declarative clients (#90)

`conformance-java` gains `example.declarative` (Feign with and without `url`, a Retrofit
interface built by `Remotes`, shadowed annotations and `@RequestLine`, which give nothing);
`conformance::java` keeps the export and `conformance-java.context-feign.txt`. The
`xlang-java-api` fixture calls the TypeScript provider through two Feign clients, and
`xlang-java.trace-up-feign.txt` is the trace up from the route through the interface method.
`freshness` edits a Retrofit base and a Feign `path` and compares with a cold index.
`xlang::a_declarative_client_method_is_an_open_item_at_its_definition` adds a mirror of the web
app (`common::xlang_java_mirror`), so the Feign client without `url` is ambiguous, and a
Retrofit method with `@Url`, which is unresolved: both are open items at the method, in
`search` and `meada decide --list`; `decider::the_excerpt_of_a_declarative_client_method_is_the_method`
checks the excerpt is the method.
