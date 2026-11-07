# JVM artifacts

- JVM artifacts (spec #26, ticket #93): `package/maven.rs` implements `Ecosystem` for
  `pom.xml`, `settings.gradle{,.kts}` and `build.gradle{,.kts}`. Grammar-independent readers
  in `jvm/{maven,gradle}.rs` are shared with Kotlin: the Maven reader takes direct project
  dependencies and literal parent group/version values; the Gradle scanner separates quoted
  strings, comments and block boundaries, without loading a grammar. `Summary::build` holds
  only literal group/project/include metadata, which `inherit` joins with build files.
  Each named manifest provides `package::maven::<group>:<artifact>`. Dependencies use the
  existing package reason with `range unchecked`; properties and variables stay unevaluated.
  The generic `fits` question matches candidate names, and `uncertainty` keeps a computed
  group or artifact ambiguous even with one provider; a computed version alone stays exact,
  its range as written. Literal coordinate fragments filter those candidates, a coordinate
  with none (`*:*`) is no consumer, and a manifest is never a candidate of its own
  dependency (`consume` and `link` drop it). Gradle map notation (`group:`/`name:`/`version:`)
  is read as a coordinate; a call (`project(...)`, `platform(...)`) and a configuration
  block's contents are not. A `${name}` or `$name` the script leaves open is kept as the entry's
  `template` and a catalog accessor (`libs.models`) as a placeholder entry (`via` `catalog
  libs.models`, name `*:*`); `Maven::inherit` (`gradle_holes`) fills them from the
  `gradle.properties` of the repository root, the settings file's directory and the module's (the
  module's wins; a name the script assigns is never filled) and from the catalog (`gradle/
  libs.versions.toml`, or the file a settings `from(files(..))` names for that accessor name),
  and drops a placeholder the catalog does not resolve. Both files are manifests that own
  nothing (`Maven::owns`). A settings assignment to a project's `name` or
  `projectDir`, and a `group` set in `allprojects`, drop the affected provider.
  Dependencies outside the workspace add nothing.
  Imports (ticket #94): `Maven::imports` turns a Java call into `(class, name(arity))`: the
  type the call's head names (`Code::outside`, which also brings the candidates of on-demand
  and static imports; a JDK `java.` class is none), else the type the source writes for its
  receiver (`Code::origins`, `Follow::Declarations`); a constructor is the class's own name.
  Each on-demand candidate is its own pair (`Outside::spellings`); `import` groups one site's
  pairs and lets none enter when two name a class some provider declares
  (`Ecosystem::declares`), as `Outside::candidates` and the HTTP kinds' `one()` read them.
  It reuses the package path: `Packages::extract` stores the `import` row, `import` resolves it
  through `Maven::resolutions`, which looks the class up in the provider's `exports` rows
  under its qualified name (the row whose `name` is its `target`), keeps the files the artifact
  owns (`Provided::owned_source`) that are no test sources (`tested`: under a
  `Summary::test_roots` root and no deeper `main` one), and picks the symbols named so (`Classes`,
  built once per provider) that fit the argument count. A method is looked up through the
  class's family, as Java merges overloads: the provider's `families` rows
  (`Resolver::ancestries`, resolved in the provider) give the ancestors in method resolution
  order, each signature comes from the first class that declares it (an override hides the
  base's), and an opaque ancestor or a family not known keeps one target `inferred` (the strict
  "Opaque outside" rule of #85, issue #353). A constructor is the class's own, never a method
  named after it (`#method`). `exact` for one target of one provider, `ambiguous` for several
  (overloads, or several providers declaring the class, whether or not they declare the
  method), `How::Package`. The Gradle scanner
  keeps the literal `srcDirs` and `srcDir` of `sourceSets.<set>.java` as `Summary::roots`
  (`test_roots` for a set other than `main`); a directory that is computed, has `..`, or sits in
  a block that configures other projects (`allprojects`, `subprojects` and their `.each`,
  `project(':x')`, `configure(..)`) or a conditional is not kept; `named("set")` and
  `getByName("set")` name the set by their literal. The source directories past `MAX_ROOTS` are
  dropped before the defaults join, so the test roots are always among the kept ones.
