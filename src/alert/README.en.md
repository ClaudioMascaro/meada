# Alerts and incident forwarding

Static reading of monitoring rules and routes, incident configuration and Alertmanager receiver forwarding.

## Interface

The reader summarizes rules, routes and receivers from configuration files. Given a route and a rule, forwarding produces events for known receivers. `[config.rs:read]` `[incident.rs:terraform]` `[forwarding.rs:events]`

## Invariants

Regular expressions respect the supported Go/RE2 subset and cost limits. A PCL condition outside the literal subset stays unknown. `[regexp.rs:compile]` `[pcl.rs:parse]`

A PCL condition with more than 64 nested groups or unary operators remains unevaluated. `[pcl.rs:ALERT_PCL_DEPTH_MAX]`

## Side Effects

Analysis produces summaries and events in memory. Forwarding substitutes supported templates without executing them or sending notifications. `[config.rs:Summary]` `[forwarding.rs:events]`

## Verification

From the repository root, run `cargo test -p meada --test it alerts::`.
