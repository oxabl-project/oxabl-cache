# 0007: No per-branch manifests

Status: accepted

## Context

Per-branch or per-commit manifests were allowed as an optimisation.

## Decision

None are built. oxabl computes every compile key locally, and a 20,000-key lookup
takes ~0.08 s in plan mode. A manifest would save one 640 KB request, at the cost of
a new record type, its retention rules, and a name or id the server could correlate
with branches.

## Consequences

The content-addressed entries are the only source of truth.
