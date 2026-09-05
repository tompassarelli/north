---
name: greenfield-reference
description: Dependency-selection rationale, evidence fields, freshness exceptions, and alternatives.
---

# Greenfield choices: full notes

## Establish the decision

Choose technology for the actual product boundary, not for novelty or an
imagined future organization. Source-authority policy still decides the owned
language; a runtime preference does not waive it. Existing repository
conventions are useful evidence, not automatically the right greenfield choice.

Compare the leading conventional choice with one meaningfully different viable
alternative. A comparison earns its cost by exposing a consequential tradeoff;
do not manufacture a large candidate matrix.

## Evidence worth collecting

For a serious candidate, retain the facts that can change selection:

```text
Official project and registry identity:
Stable release/date; supersession or deprecation:
Maintenance and adoption evidence:
Direct/transitive dependency and install-script exposure:
Build-time network needs:
License and distribution duties:
Maturity at the required seam:
Behavior we own versus delegate:
Exit or replacement path:
```

“Production maturity” is evidence about the dependency, not permission to impose
production ceremony on the consumer. Prefer concrete maintenance and use
evidence over download counts alone.

## Freshness and exceptions

Apply the distilled guide's freshness hold. An exception needs the exact
security exposure or blocking requirement; release excitement and uncertain
future benefit are insufficient. Verify that the proposed release actually
addresses the exception.

## Alternatives and stopping point

A platform feature may avoid a dependency; a mature library may avoid owning
incidental complexity. A small implementation can be right for core semantics
or a genuinely bounded gap. Stop when constraints and one discriminating check
select the smallest credible route; reconsider when those facts change.
