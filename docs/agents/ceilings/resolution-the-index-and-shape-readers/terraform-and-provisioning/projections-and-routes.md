# Projections and routes

- GCP name projections and Cloud Run container summaries (#667) reuse the evaluator work,
  root and repository byte budgets, plus `ENVIRONMENT_BYTES_MAX` for each workload. Projection
  evidence is charged before copying, including added instance provenance. Subscription
  pattern evidence is checked before adding its canonical prefix; the prefix cannot supply
  the physical name’s minimum literal evidence.
  Structural scans charge retained block metadata to `EVAL_WORK_MAX` before walking or
  collecting children, including empty bodies. Exhausting a workload's environment byte
  budget clears its entries while preserving charged images from its containers.


- IaC HTTP route summaries (#666, `http/iac.rs`): `IAC_HTTP_STORED_MAX` is `4 * lang::MAX_BYTES`
  (4 MiB) per config file, counting repeated contracts/handler names before copying and the
  complete serialized result. Overflow drops the whole summary. Route facts share the IaC
  root/file budgets; attribute aliases and REST parent/method chains share `EVAL_DEPTH_MAX`
  and the evaluation work budget. Only finite API Gateway v2 HTTP routes/explicit quick
  create and v1 API/resource/public-method joins to a proven Lambda are read. WebSockets,
  non-Lambda services, `$default`, unknown selections and incompatible APIs provide none.
  SAM requires explicit Api/HttpApi method/path fields and any explicit API reference to be
  known. Serverless reads its existing bounded event list and keeps method/path distinct;
  only a method field's `*` means `ANY`. Handler validation reuses `image/code.rs`, with a
  unique supported local module or explicit source file and an existing stored symbol;
  archives and unsafe/ambiguous paths use the repository. Provider resolution never validates
  paths during queries; existing code rendering may read source for line displays.
