# a7-sls

The Serverless Framework probe of prove-meada (#654), the shapes of the `serverless-nodes` test
fixture. No oracle reads them yet: `prove.py values` checks their config values, and the IaC
answers (`search` kind `iac`, `context`, `trace`) are checked by hand against the cases below.

- `api/serverless.yml`: `${self:...}` through two levels, `${opt:stage, 'dev'}` (overridable),
  `${env:...}` and `${ssm:...}` (unresolved parts), HTTP, `httpApi`, SQS, SNS and schedule events,
  an event `arn` by `!GetAtt`, a `resources` section with queues, a table, a bucket and a secret
- `worker/serverless.yml`: `service` as a mapping, a literal stage, the `GET path` shorthand
- `misc/serverless.yml`: no `service`: no node
