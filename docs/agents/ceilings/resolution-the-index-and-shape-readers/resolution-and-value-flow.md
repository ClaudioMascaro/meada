# Resolution and value flow

`Code::value_within_at` (#668) folds one selected identity argument with exact return-call
frames and a kind-supplied typed-construction predicate. It uses the existing evaluation
limits and does not change ordinary `Code::value`. Shared SDK evidence memoizes normalized
frame argument names and client evidence; unknown alternatives invalidate the result. Go
records only guarded first-result call ranges for the shared SDK evidence walk; ordinary
scalar folding and error/other result bindings remain unknown. These facts follow the full
value cache, not the lean index, and reuse the nearest lexical binding query.
Scoped selector effects (`resolve/mutations.rs`) follow exact aliases and argument frames,
compare actual read/write call order, and share selection/memo normalization. A write inside a
loop that also holds the read comes before it (`Facts::loops`: the outermost `for`, `while` and
`do` statements of TypeScript/JavaScript, Python and Go; a comprehension, a callback such as
`forEach`'s and a recursive call are no loop). The rule is lexical: an object made inside the
loop, or a `break` or `return` right after the write, still names nothing, and a function that
calls and then writes keeps its name when a loop of its caller calls it. A computed key or a
spread names its fields once folded, through spreads of spreads: a write to one of them changes
it. A recursive call through a written parameter anywhere in the group refuses all write
knowledge, so every named request object of the group is unresolved. A write Meada cannot tie to one object (through a parameter of a function it finds
no caller for, which a function value may still receive, or a member path such as
`cmd.input.Bucket`) changes that field of every object its scope can reach: a module-level
object anywhere in the file, a function's own object only in that function. Retained write
facts charge their names and scopes to `MAX_BYTES` (1 MiB); overflow removes complete write
knowledge rather than keeping a partial list. Indexing, alias expansion and querying charge
`EVAL_BUDGET` and `EVAL_DEPTH`. Missing facts/calls, opaque aliases, cycles or exhausted work
cannot prove an object unchanged. A write to a shared default object is conservative across
invocations; a constructor with no honest source call site uses all-writes mode. Standard
mapping effects are supplied by `contract/sdk/mutations.rs`, never cloud policy in a pack.
Typed helper subtypes stay in kind-owned identity and complete memo keys until projection;
an incompatible helper stays unresolved. C# async SDK factories require one await; a bare
Task or an already-awaited client awaited again supplies no receiver evidence.

- `names.rs`, `http/csharp.rs`, `channel/java.rs` (#466): two cases are not pinned.
  (1) #358: a Rabbit `convertAndSend("orders", o)` whose `o` is a parameter of a type the
  repository does not declare (`void f(Order o)`) reads as the named producer `amqp orders
  producer`, because Java's implicit `java.lang` on-demand candidates make the type an outside
  one; #358 asks for a producer with no name. `an_argument_of_unknown_type_does_not_tell_the_overload`
  fixes the observed answer, and which is right waits on the maintainer.
  (2) #354: the C# HTTP reader reads its attributes through `names::known` (the value reading),
  not `names::annotation` (the decorator reading, which a class with an outside base does not
  hide an import from); the test in `http/csharp.rs` pins `annotation` over C# facts, not that
  reader.

- `resolve.rs` (`Resolver::may_declare`, ADR 0019): a base among a pack's known outside members is
  matched by its spelling, so a same-named type of a wildcard-imported outside package that
  declares an overload of the called name may give a wrong `exact`; a bare call inside the type,
  a call with a trailing closure or one without labels compares a Swift entry by its base name; an extension
  the tables do not list, in scope of a call no own overload fits by type, is missed. A base the
  tables do not list keeps every name opaque.

- `resolve.rs`: `TYPE_PARTS` (64) bounds the files of one unit one partial type is read across
  (`Resolver::parts`); classes of one name in more files are each a class of its own, so a
  member lookup reads its own file and a name binding them all is `ambiguous`. A C#
  conformance test writes one type in 64 files and in 65. Definitions of one name in files of
  several units (C# projects) bind, from a file of one of those units, to its own unit's
  (`Resolver::in_unit`, at the end of `resolve_as`), as the compiler prefers the project's type
  to a reference's; from a file of a unit that declares none of them, the name binds to the
  one defining unit among those its project references, directly or through the references
  that pass on (`Facts::unit_references`, from the walk of `dotnet::references`;
  `Resolver::in_references`), and stays `ambiguous` with several or none, since a project
  not referenced may still be visible through references not read. The overloads of one
  family's member are kept whole, since they merge across units (a base in another project, a
  Swift extension in a test module): methods narrow only when two of their types share a name.

- `resolve.rs` (`labelled_args`): `LABELLED_CELLS` (65,536) bounds the pairs of a parameter and
  an argument the label matcher weighs for one call; a wider call fits no function that labels
  its parameters, so it resolves by name or to nothing.

- `resolve.rs`: wrapper expansion (HTTP clients, message producers and consumers) walks every
  simple path up the callers, and does not climb a call found only by a name match, so a wrapper
  method called on a receiver whose class Meada cannot tell (in TypeScript any receiver but
  `this`, a `const api = new Api()` included), or a function called by a bare name no import
  binds, is read where it stands; value flow (`Code::origins`, for a parameter's arguments) does
  not climb such a call either; a parameter captured by a closure is unknown while a different function frame is active;
  a root value walk without a bound frame may climb the outer function's exact callers.
  `Code::origins` does not follow a name its scope binds more than once, or an attribute not
  assigned a call once in its class; a nested or rest
  pattern in a destructured parameter binds nothing, and a spread in the object a caller passes
  makes the names it may override unknown. `WRAPPER_DEPTH` (64): a wrapper more calls up than
  that is read where the climb stops, its parameters unknown. `EVAL_BUDGET` (2^16 per file):
  folding values (`Code::expand`, `Code::value`) and tracing them to their origins
  (`Code::origins`) charge each step to the file whose code it reads (a value folded, a part
  copied, a call a wrapper climb reaches, a constant, parameter or return value followed, a
  composite literal `Code::field_values` reads, past which the field's value is open);
  past it, that file's values are unknown, the climb stops at its calls and no origin is found
  through it, so a string doubled through forty constants (`const b = a + a`), or a lattice of
  functions each calling or returning the next twice, ends. The rows of a DynamoDB transaction
  fold on a budget of their own, of the same size (`Code::apart`), so the other calls of the
  file keep their sides whatever the transactions spend. About 1,700 to 2,200 transaction
  calls in one file spend it (with each value held in a name, the first 1,725 calls are named
  in Java, 1,928 in Go, 2,185 in C#; a hundred calls of 25 inline entries each do not): the
  transaction calls after that have no side, and the one where it ends may carry an unresolved
  reader though it has no `ConditionCheck` (#719). A Java builder held in a `final` field is read in its own
  file, and a call on its name from another Java file leaves it unresolved; one handed to a call
  or aliased there, or reached by reflection from there, is not seen, and the field keeps its
  first name. A request type is the SDK's unless a type of its directory, an import or a
  variable says otherwise: one of the same package in another source root is not seen (#719).
  A request, a list, an item or an entry held in a name on a path of entries (a DynamoDB
  transaction's, EventBridge `PutEvents`' `Entries[]`, #773) is followed only when the function that
  declares the name mentions it twice. In Go and C#, one mentioned a third time (logged, read by two
  requests), one held in a parameter, a field or a package variable, the first name the path meets
  when any file of the repository assigns the field read through it (`q.TableName = ..` on any
  variable of any type, in any package or class; a write of another field does not count), and a C#
  request filled by property assignments have no table; nor have Go locals declared together (`p, q
  := ..`), a Go entry declared in a loop body and a Go request held by value and passed by address.
  In Java the rule covers a list or an array: one mentioned a third time or held in a field has no
  table (a `List.of` list cannot change and keeps its tables), and neither has a `new T[]{..}` array
  or a `new ArrayList<>(..)` copy mentioned twice. A method's name is no mention in Java: a list
  `entries` passed to `.entries(entries)` is mentioned once there (#773). A Java list, array or
  single entry that is the one argument of the step that sets the list (`transactItems(..)`,
  `entries(..)`) and comes in a parameter has no mentions to count and no table, one a wrapper only
  passes on included (`write(List.of(..))`, #772); two entries passed as two arguments, and a table
  name in a parameter, keep their tables. A path with no list of entries (a list of names: SSM
  `Names[]`, Secrets Manager `SecretIdList[]`) keeps the rule it had.
  `EVAL_DEPTH` (256): a value
  folded deeper than that, through the items of lists and objects, the constants it names and
  the arguments callers pass, is unknown, and a value traced through more constants,
  parameters and return values than that, each into the next, has no origin.
  `Code::eval` keeps variant-specific temporaries in separate helpers so an alias step does
  not reserve the combined object/call frame. The Java HTTP local-chain regression also runs
  in CI's debug profile: a debug0 pass alone does not establish stack safety at this limit.
  `FAMILY_DEPTH` (128): a class family (the class and its ancestors in C3 order) longer than
  that is absent, so every member of the class is unknown (a name match at most), and the
  linearization recurses no deeper down the bases. The bases count from where the walk
  started, a family looked up for a member a base names (`class Z(K.D)` linearizes `K`'s)
  included. The families being linearized are the resolver's (`Resolver::linearizing`), so a
  lookup that begins its own walk (a glob, an alias, an on-demand import, a base, a receiver)
  sees and counts them too: a `using static N.A;` read to name `A`'s base (`class A : Inner`)
  does not linearize `A` inside itself, the member it asks of `A` is answered as in a cycle, and
  the base is opaque unless another import names it.
  `LINK_DEPTH` (64): a name followed through more links than that, each into the next (a file
  re-exporting it from another: `export { x } from`, `export * from`, a Python `from m import x`
  or `import *`, a Rust `pub use`; a Rust alias, `use a as b`, or glob, `use m::*`), is unknown
  (a name match at most), and no other import, `export *`, glob or submodule binds it in its
  place. The links count across a walk begun past a glob, an alias or an on-demand import: it
  goes on from the links already followed (`Resolver::links`). A star import from outside the
  repository (`from numpy import *`) is taken to bind nothing (`ponytail:` in
  `Resolver::imported`): a name it rebinds stays linked `exact` to the import or definition
  before it, as with the standard library's accelerator modules (`from _heapq import *` after
  `def heappush`). Where a Python star import sits is not compared with
  the code that reads a name it rebinds (`ponytail:` in `Resolver::rebound`): code run while
  the module loads (its top level, a class body there, its main guard) gets the name unknown, as
  it may run before the star import, and a function's code gets the star import's, as for every
  binding a function reads. `RECEIVER_DEPTH` (64): a receiver whose class is named through
  more receivers than that, each through the next (`x: y.C`, `x = y.C()`, or a Rust `let` that
  takes the class of a call's result on the next, `let x = y.f()`, `Class::Returned`), holds an
  unknown class.
  The receivers and the calls whose result is being read count from where the walk started, and
  across a walk begun past a glob, an alias or an on-demand import: the count is one for the
  whole lookup (`Resolver::chain`, as `Resolver::links` is for the links), so a chain that
  alternates receivers and imports costs its length, not `RECEIVER_DEPTH` per link up to
  `LINK_DEPTH`. A receiver or call met again is found in its own `Work` alone, and so are the
  families linearized so far: an import's lookup begins with none. When `FAMILY_DEPTH` or
  `RECEIVER_DEPTH` cuts a walk, or `LINK_DEPTH` cuts one begun past an import, the families
  being linearized give up and none is kept: each is linearized again when asked for, and does
  not depend on the walk that first reached it.

- `resolve.rs` (`Code::field_read`, #687): a Go struct field holds an environment read only when
  every composite literal that sets it gives one and the same read (`Env` or `EnvOr`, nothing
  around it), a constructor's parameter through every exact caller; a literal string, a read
  with a suffix (`os.Getenv("X") + "/v1"`), a constructor with no caller or that any call
  reaches by a name match, a constructor method another definition shares the name of (an
  interface's, whose calls dispatch to it; no pack marks interfaces, so any other counts), and a
  field some file writes through a selector of any type (`x.base = ..`, matched by the field's
  name alone) leave it unknown, as do the literals `Code::field_values` cannot read (by
  position, an alias's, a conversion). `FIELD_DEPTH` (16): a field folded through more fields
  than that (`&T{base: o.base}`, each from the next) is unknown, and the values folded on the
  way share one `EVAL_DEPTH`; the first depth a field is folded at is the one kept for it, so a
  field first met deep in a chain stays unknown for every later read. The constructors climbed
  are bound by `WRAPPER_DEPTH` and a constructor met again leaves the field unknown. A
  constructor read as a value (`mk := New; mk(x)`) is not seen: its exact callers alone give the
  field.

- `resolve.rs`, `lang/ts.rs` (#536): a call folds to the string its function returns
  (`Code::built`, `Def::lone_string`) only for TypeScript and JavaScript (the other packs leave
  `lone_string` empty), for a `function` declaration or an arrow or function expression with an
  expression body or a body of one lone `return`, not `async` or a generator, whose value is a
  string (a template, a literal or `+`), and for a call that resolves `exact` (a name match may
  reach another function); the parameters bind to the call's arguments, so the fold costs the
  `EVAL_DEPTH` and `EVAL_BUDGET` of the file it reads. Misses, the URL staying an unresolved
  consumer: a `return` after other statements (a local, a guard, a `const`), a returned
  value that is not a string (a call, a conditional, a member), a class method (`lone_string` reads
  functions and arrow or function expressions, never a `method_definition`), and a string with a part that does not
  fold, an environment read (`process.env.API_URL`) or a build-time global included: the builder
  gives up as a whole where a template written in the `fetch` call keeps the environment read and
  links by `base_url_env`, so a base that is a conditional over a build-time global, as
  spring-petclinic-reactjs writes it, stays a separate gap.

- `resolve.rs` (`Code::origins` with `Follow::Constants`): only `const` bindings with a plain name are followed; a client
  held by `let`, a class field, `this.x`, a destructuring or a parameter, or chained without a
  binding (`new Kafka().producer().send`), is not recognized (no marker).

- `resolve.rs` (`Resolver::is_self_or_ancestor`, #438): a Rust glob reads the private names of
  the importing file or of an ancestor file on its `super` chain; an ancestor written as an
  in-file `mod` (`use crate::a::outer::*` from `a::outer::inner`) is read as a foreign module, so
  only its public names come through the glob.

- `resolve.rs` (`Resolver::owned_outside`): a Rust method called on a call chain
  (`client.get(u).send()`, `Call::root` = `client`) resolves to nothing when `client.send` would
  be external, and so does one on a chain that starts at a call (`rocket::build().mount(..)`,
  `Client::new().get(u)`, no `Call::root`) when that first call, the innermost one starting
  where the chain does, is external (`Resolver::rooted_outside`, Rust only); a root that is a
  repository type, a parameter, or a local with no written class (a client a repository
  function returns) is name-matched as before. `self` and `Self` in an `impl` of a type from
  outside the repository (`impl ApiExt for reqwest::Client`) are opaque too
  (`Resolver::outside`): `self.clone()` there resolves to nothing.
