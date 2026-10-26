# 18. A C# extension method called on a receiver feeds the value flow

- Status: accepted
- Date: 2026-10-26

## Context

The value flow that finds routers, groups and clients (through constants, arguments and return
values) climbs `exact` hops alone: a call resolved only by a name match (`inferred` or
`ambiguous`) may be made on any object or reach another function, so it never feeds a contract
(`CODING_STANDARDS.md`, "Name-matched hops", under "Certain or absent").

Spec #28 asks, in ticket #110, that ASP.NET Core minimal API prefixes follow the route builder
through extension method receivers: `group.MapOrders()`, with `MapOrders` a
`static void MapOrders(this RouteGroupBuilder g)` of another file, serves its routes under the
group's prefix. That form is how real services split their endpoints across files.

The resolver does not look C# extension methods up. A call `x.M()` on a receiver whose class
it cannot place, or whose class is outside the repository (`RouteGroupBuilder`), stays a name
match even when the compiler would bind it to the one extension method the file sees. Resolving
extension methods in the resolver would need the receiver's static type for every call, which
Meada does not read for outside types.

## Decision

- A C# call made on a receiver (`x.M()`) whose unique name match (`inferred`) is an extension
  method feeds the value flow when the compiler would bind it as one: the method takes its
  receiver as its first parameter (`this`, `Binds::Instance`) and the static class declaring it
  is in scope of the call by its simple name (its namespace is the call's own, one around it, or
  imported by a `using`). `Resolver::extension_call` decides this, `Resolver::extension_callers`
  holds the calls it accepts, and `Resolver::flows_to` gives the value flow either an `exact`
  target or such an extension target.
- The link itself stays `inferred`: the call's level in the export, `context` and `trace` does
  not change. Only the value flow takes the hop.
- Only a pack that declares `LanguagePack::extension_methods` (C#) takes this path; every other
  pack's value flow still climbs `exact` hops alone.

## Consequences

- A minimal API route mapped by an extension method on a group gets the group's prefix, and a
  method called on two groups serves under both.
- An outside type's instance member of the same name as a repository extension method is
  missed: C# prefers the member, and the value flow takes the extension. This is a known
  ceiling (`docs/agents/architecture.md`), rare in practice, and the reason the rule is kept to
  a unique name match with the declaring class in scope.
- Two extension methods of one name in scope (an ambiguous match) feed nothing.
