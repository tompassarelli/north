---
name: staffing
description: Select and instantiate the right worker or orchestrator profile after orchestration has chosen the task shape, grade, domain requirements, topology, semantic tier, deliberation, and posture. Use when choosing a stock agent template, applying a justified template override, or composing a bespoke role contract.
hooks: [agent-spawn-guard]
agents: ../agents
---

# Staffing

Staffing turns an orchestration decision into a concrete role profile. The
canonical selection laws live in
`~/code/north/main/orchestration/doctrine.md`; do not recreate them here.

Use the stock profiles in `~/code/north/main/orchestration/agents/` when their
responsibility, deliverable, topology, and capability boundary fit. Read the
source contracts under `repo:orchestration/docs/` and the stock inventory in
`repo:orchestration/staffing/catalog.json` when a selection needs detail. A
changed topology, responsibility, deliverable, capability boundary, done
criteria, or report shape is a bespoke composition rather than a template
override.

Staffing decides who should do the work. Durable ownership of a particular
thread is a coordination assignment and belongs to the `assignments` module.
