---
name: production-hardening-reference
description: >-
  Inactive detailed reference for production-hardening-distilled. Use only when
  that workflow requests its assurance worksheet or failure-path checklist, or
  when the user explicitly requests production-hardening-reference.
---

# Production-hardening reference

## Assurance envelope

```text
System and scenario:
Guarantee sought:
Allowed degradation:
Inputs or actors considered hostile:
Load and timing assumptions:
State that must survive:
Recovery or rollback requirement:
Evidence that will decide the claim:
```

Distinguish existing guarantees from desired guarantees in the completed
envelope.

## Failure-path checklist

Select only entries that bear on the named guarantee:

- trust and authorization boundaries;
- limits, queues, backpressure, timeouts, and retry amplification;
- races, ordering, duplicate delivery, partial completion, and cancellation;
- atomicity, corruption, restore, migration, and version skew;
- capacity and latency under a representative workload; and
- operator detection and the documented recovery action.

Put a durable guarantee in the system's existing contract or test surface. A
one-off review does not need a separate attestation system.
