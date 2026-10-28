# 19. Known outside members

- Status: accepted
- Date: 2026-10-28

## Context

Java, Kotlin, C# and Swift merge overloads across a class hierarchy: a method a class declares
does not hide a base's method of the same name, and the compiler picks among all of them by
argument types (or labels). A base from outside the repository is opaque (`CODING_STANDARDS.md`,
"Opaque outside"), so Meada cannot know whether it declares an overload that fits a call better:

```java
class Names extends ArrayList<Item> {
    boolean add(String s) { return add(new Item(s)); } // Java calls ArrayList.add(E)
}
```

Narrowing by argument count alone would link `add(new Item(s))` to `Names.add(String)` at
`exact`: a wrong `exact`, the worst failure in the coding standards. So the resolver applies the
probe strictly: an opaque ancestor anywhere in a family (a base that resolves to no repository
class, or in Swift a block that conforms to an outside protocol) keeps the family's own members
from `exact`, and a call to a name the family does not declare is unknown.

The cost is paid by every type with an external base. `implements Serializable`, `: Codable`,
`: IDisposable`, `: Hashable` and Vapor's `RouteCollection` are among the most common bases in
real services, and most of them declare one or two members, or none (issue #353).

The options weighed:

1. Keep the strict rule.
2. Treat the outside types whose members are a fixed fact of the platform as transparent for
   every name they do not declare (`java.io.Serializable` declares nothing, `Comparable`
   declares `compareTo`).
3. A declared allowance: a `meada.toml` key, or a pack table, listing the external types whose
   methods are known. Heavier, and it drifts from the platform version.
4. The own class wins: accept the rare wrong `exact`.

## Decision

Option 2, generalized to the four packs that merge overloads.

- Each pack declares a table of known outside types, `LanguagePack::known_outside`: the
  qualified name of each type and the member names it declares, empty for a marker. A name is
  spelled as the pack spells a member (with its labels in Swift, `hash(into:)`). The `Resolver`
  of a pack reads its table with the tables of the packs that resolve with it (Kotlin's, through
  Java's: `lang::known_outside`).
- A family is opaque for a called name only when some outside ancestor is unlisted or declares
  that name (`Resolver::may_declare`, read by `Resolver::opaque`, `overloaded`, `member`,
  `inherited` and `inherited_field`). When every outside ancestor is listed and none declares the name, the
  family resolves as one without bases: a call to an own member narrows by argument count (and
  labels) and lands `exact`, or `ambiguous` among the family's own overloads; a name neither
  the family nor the listed types declares resolves to nothing. This holds through lineage and
  for the Swift members of an extension block that conforms to a listed protocol.
- A called name a listed type declares (`compareTo`, `description`, `Dispose`, `hash(into:)`)
  keeps the strict rule, and so does any family with an unlisted outside ancestor (`NSObject`,
  `ArrayList`, `Exception`, `JpaRepository`, `ControllerBase`, `View`).
- An opaque ancestor is spelled as the file's imports spell it (`Resolver::imported_name`):
  qualified under an explicit import or when written qualified, bare under a wildcard or
  implicit import (`java.lang`, `import java.io.*`, `import Foundation`, `using System`); a
  generic base without its arguments. It matches an entry by the full qualified name, or a bare
  name by the entry's last segment.
- A name an extension in scope of every file adds to a listed type counts as one it declares:
  a call no own overload fits by type takes the extension, and Meada narrows by count alone.
  Kotlin's standard library adds `use` to `Closeable` and `AutoCloseable` and `coerceIn`,
  `rangeTo` and their kin to `Comparable`; Foundation adds `localizedDescription` to Swift's
  `Error`. C#'s `IEnumerable` stays out of the table: the LINQ extension methods every file sees
  through the implicit usings (`Contains`, `Count`, `First`, `Select`) are many and grow with
  .NET, and a collection class often declares one of those names.
- The initial tables hold the JDK markers and `Runnable`, `Comparable`, `AutoCloseable` and
  `Closeable` (Java), `kotlin.Comparable` and the standard library's extensions (Kotlin, with
  Java's), `IDisposable`, `IAsyncDisposable`, `IEquatable` and `IComparable` (C#), and the Swift
  standard library's marker and small protocols, the raw value types an enum may carry and
  Vapor's `RouteCollection` (Swift). A table grows with evidence from the dogfood repositories,
  never by guess. Vapor's entry is the one library type in a pack's table: its one requirement
  (`boot(routes:)`) is as fixed as the standard library's, and the Vapor kind cannot weigh a
  family's members.

## Consequences

- A class whose outside bases are all listed resolves calls to its own members at `exact`,
  which the value flow then climbs: a helper a Vapor `RouteCollection` calls from `boot` serves
  its routes under the collection's prefix.
- Accepted risk: a same-named type from a wildcard-imported outside package (`import com.x.*`
  with a `com.x.Runnable`) that declares an overload of the called name. Its bare spelling
  matches the platform's entry, and the call may land a wrong `exact`. A qualified spelling (an
  explicit import or a written qualified name) never matches another package's entry. In Swift
  (every import takes a whole module) and in C# (`using System;`) the bare spelling is the
  usual one, so there the risk is any imported module or namespace that declares a type of a
  listed name (an `Error` or a `RouteCollection` of its own) with an overload of the called
  name.
- Another accepted risk: an extension the tables do not list, in scope of a call (a library's
  extension of `Comparable` or `IDisposable`), that a call no own overload fits by type takes.
- A Swift entry is compared without its labels, since the resolver looks members up by base
  name: `hash(into:)` keeps every call named `hash` at today's level, a stricter reading than
  the table spells.
- Storing arities for listed names, so that an own overload of a listed name could be `exact`, a
  `meada.toml` allowance and "own class wins" stay out.
