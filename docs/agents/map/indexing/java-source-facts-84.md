# Java source facts (#84)

`lang/java.rs` reads `.java` with tree-sitter-java, with no JDK or build tool. Types carry their
package-qualified names; methods and constructors carry the parameter types as written.
`Facts::overloads` maps method names to their definitions with signatures; `Facts::constructors`
keeps constructor families under their class names. The shared resolver selects the family by
the stored `Call::constructs` and narrows it by `Call::arity`, so cached and cold resolutions
agree. Comments do not count as arguments, and type-use annotations and comments do not enter
written signatures; literals recovered from malformed types are excluded too.
`members_in_scope` admits bare class members, and `Imported::Qualified`
resolves a class import to its full name while preserving the exact outside spelling.
`qualified_types` admits a fully qualified repository type without an import only when lexical
lookup leaves the path head unbound. Enum and interface constants bind values too.
Constructor calls read the type namespace, so methods and values of the same name do not hide
an imported class. Only class candidates can supply constructor families, including after a
static wildcard import yields several bindings. A method named after its own class has `#method` after its written signature,
even when that class declares no constructor, keeping its identity distinct and stable when
a constructor is added. Constructors keep their class-named signatures.
Package groups come from `Facts::package`, independently of directory layout. The package's
inside glob sees its non-public types; public star sets and hidden members define the exported
surface. Static class globs take `Facts::statics` alone, and a pack whose `static_imports_inherit()` is false (C#: "inherited members aren't imported") offers only the statics the named class declares itself, not its bases'. `unordered_globs` combines candidates from
on-demand imports, explicit imports take priority, and the current package takes priority over
public package globs including `java.lang`. A shared package checks each declaring file's star
set, so its first file does not hide another file's public types. Qualified imports go through
shared exports, preserving duplicate fully qualified types as ambiguous candidates. `meada-core::parent` ignores dots
inside signatures when finding the enclosing scope. Calls in lambdas and anonymous classes
stay with the enclosing definition. Generic annotation calls carry `decorates`, positional
values and named arguments for the contract kinds to read. The annotations of a field or a
parameter, which no definition carries, are `Facts::annotated` (transient: the binding they
mark and the annotation as a call), and every name a file assigns (`f = v`, `x.f = v`) is a
marker constant `.f` (stored, like Go's), which `Code::field_written` reads (#89). A `static`
field's annotations are not recorded. A `final` field a constructor assigns keeps that
constructor as `Const::scope`. `this(..)` and `super(..)` are constructor calls
(`Call::constructs`) of the class's own name and of its `extends` type.
