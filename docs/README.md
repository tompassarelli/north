# North docs

The reading order for a newcomer is the numbered list in the [README](../README.md). This page is the last step of it, the reference: where to look a fact up once you know what north is. Nothing here is needed before the first landed change.

1. [Terms](terms.md): the four layer terms.
2. [Architecture](../ARCHITECTURE.md): the seven layers, every crate, the life of a task and the rules.
3. [Onboarding](onboarding.md), [briefing](briefing.md), [landing](landing.md) and [the operator repository](operator-repository.md): how north is installed, how a worker is staffed and how its change lands.
4. [Commands](../README.md): `north --help` lists the operator and agent commands, `north --help-all` every command with its contract, and `north COMMAND --help` one command.
5. [Feature records](features/): one record per behaviour, grouped by job in the order `north --help` groups its commands; `north docs features-index` writes them as one untracked page, the features index.
6. [The fact graph](graph.md): the terms, the ask query, ingest, retention and the ready set behind `north graph`.
7. [Feedback loop](feedback-loop.md): each recorded signal, its writer and its reader.
8. [Outside sources](references.md): what each source shaped and what came of it.
9. [Rationale](rationale/README.md): why north is built this way; dated plans, measurements and decisions, not needed to use it.
