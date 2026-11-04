# Java receivers and supertypes (#85)

Each class writes `Def::bases` from `extends` and `implements` (type arguments dropped; an
unreadable base is `(type)`, opaque), and each method its declared return class as
`Def::result`. Fields, parameters and locals become `Facts::receivers` with `Class::Type` of
their written type; `var` takes the class of `new T(...)` without a body, or the result of
a call (`Class::Result`, the initializer call's place). A chained call (`make().save()`)
writes `Call::on`. Primitives, arrays, varargs and type parameters are `Class::Unknown`. A local is
typed only when it is the one binding of its name in its method scope, not when a field of the
file has its name (a use before the declaration reads the field), and not in an initializer
block or a field's lambda. The pack sets `base_receiver` (`super`), `unordered_bases` and
`implicit_members` (`java.lang.Object`'s methods). The shared resolver reads overload families
per signature across the C3 family (`Resolver::overloaded`, strict about opaque bases), bare
calls through `Reading::Method` (`Resolver::inherited`), bare fields through
`Resolver::inherited_field`, and name matches through the owning family (`Resolver::matched`).
Fields and methods are separate namespaces where class members are in scope: a name read, not
called (`Reading::Read`: the head of `repo.save()`, a member read on the way in
`this.repo.save()`, and every value `Code` folds or traces), sees the field `repo`, never a
method `repo()`, while a call (`repo()`, `this.repo()`) sees the method (#89).
