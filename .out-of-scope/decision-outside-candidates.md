# Decision outside the candidates

Meada does not accept an `ambiguous-link` decision whose target is not one of the item's
candidates. Replay reports it as a stale decision with the cause `not-candidate` ("target no
longer a candidate"), the item stays open, and `meada decide` offers only the candidates, at the
prompt and in decider mode.

## Why this is out of scope

The candidates of an ambiguous link are the providers of the contract Meada read at the call
site. A decision picks one provider of that contract: it answers "which repository", never "which
route". A target outside the candidates is a provider of another contract, so accepting it would
let a line of `meada.decisions.toml` replace what the call site says:

- The link would rest on nothing Meada read. Replay could not hold it to the call (the cause
  `not-candidate` exists for exactly this), so it would not go stale when the code changes, and a
  typo in a target would become a link at the level `decided`.
- It would hide the real defect. When every candidate is wrong, Meada read the call wrong, and
  the fix belongs to the reading.

That is the case behind the request. In the 1.3.0 dogfooding report a Backstage client built its
base through a helper method, value flow lost the base, and the call read as `GET /items`:

```
example.com/acme/portal::plugins/catalog-backend-module-orders/src/client.ts::OrdersClient.listViaHelper::fetch#0
plugins/catalog-backend-module-orders/src/client.ts:19  fetch(`${base}/items?limit=10`)
  1) example.com/acme/svc-a GET /items
  2) example.com/acme/svc-b GET /items
```

The right route, `GET /api/orders/items`, is served in the workspace and is not offered. A
decision that names it is refused, and it should be: the item is the symptom. #801 tracks the
cause (at a limit of value flow the call becomes an open item whose candidates end with the known
part, so the right route is among them), and #810 and #811 lift the two limits themselves.

What already exists: a `dynamic-url` decision names any contract a repository serves, because a
dynamic URL has no contract of its own to hold the decision to. Whether that target should be
checked against the item's candidates is #806. A decision of the wrong kind on an open item is
reported in misleading words, which is #802.

What does not work until #801 lands: an ambiguous link whose candidates are all wrong cannot be
decided at all. The reporter's workspace had two such items.

## Reopening

Reopen from a real workspace, not a fixture, with an ambiguous link whose contract Meada read
right and whose candidates still miss the provider a person knows is the one. Attach the item's
block from `meada decide --list` and the `meada export` lines of the consumer and of the
provider. A case that comes from a wrong reading of the call goes to an `inference` issue instead.

## Prior requests

- The 1.3.0 dogfooding report of 2026-11-11, items 3 and 4, triaged into #801, #802 and #806. The
  request itself was not opened as an issue.
