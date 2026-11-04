# Java conformance (#84)

`conformance-java` holds Maven modules and a Gradle source tree, with `example.clients.Injected`
(Spring `@Value` with a literal, a placeholder the module's configuration holds and a placeholder
with a default), a computed Gradle coordinate and, in the Gradle build, a catalog accessor and a
`gradle.properties` hole (#467). `conformance::java` checks
all Java definitions and its golden export; the companion engine tests check package and
static imports and their priority/ambiguity (including duplicate fully qualified types),
overloads and constructors in separate namespaces, fully qualified calls with shadowing,
class-named method identities, qualified parameter types, visibility, anonymous
class and lambda ownership, and search/context goldens. `index` checks byte-identical exports
from independent copies; `freshness` edits, adds and deletes sources and compares the refreshed
export with a cold one, including overload arity from an unchanged file's cached facts.
The hostile-depth tests exceed Java's definition, value and member-path limits and reject
exact links that escape an omitted scope. A long package prefix exceeds the qualification
budget and leaves the whole file out; annotation values, comments and malformed type literals
never enter exported names. `schema` has a Java sample for the adapter facts the index persists.
The `example.channels` package (#92) holds a client of each broker kind (Spring Kafka, Spring
AMQP with a listener's and a configuration's bindings, JMS, SQS, jnats, Jedis and Lettuce), a
topic from a `${key}` placeholder and one that cannot be evaluated;
`java_channel_clients_link_their_producers_to_their_handlers` checks every link and the
unresolved sides, and `freshness` edits the configuration key and a topic constant.

The `example.calls` package (#85) adds `JAVA_CALLS`, each receiver form at each level: field,
`this.field`, parameter, local, `var` with `new T(...)` and with a call's declared return type, a
fully qualified field type, an interface receiver, a parameter and a type-named local that
shadow, inherited overloads of another arity, a written-signature override, a same-count tie
across the family, an outside base, and unknown receivers matched in the calling family or
across the repository; `conformance-java.context-ping.txt` lists exact, inferred and ambiguous
callers, with a narrowed match's "one of several" reason; the fixture also has a chain and a
`super` call. Resolver unit tests (`java_calls`) cover strict outside bases (`ArrayList`'s
`add`), Python's first-defining-class rule, a redundant interface and its subclass, name matches
through the receiver's family and never the caller, an inherited field against an outer one,
chains and `var` through the overload called, `super`, initializer locals, `Object`'s implicit
members, and receivers left unknown: a type parameter, a local named like a field, same-named
locals of different types, an anonymous class and a cyclic family. `java_static_imports_an_outside_base_may_shadow_are_name_matches` keeps a static import `exact` in a plain class and
`inferred` under an outside base.
