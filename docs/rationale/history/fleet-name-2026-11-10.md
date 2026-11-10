# Fleet replaces the old agent names

Tom decided on 2026-11-10 that north's agents are north's fleet, and retired the two old names (north#1058). The fourth layer in [terms.md](../../terms.md) is now **fleet**.

The retired names are "Socrates" and "workforce".

- **The GitHub App.** The app that pushes and comments as an agent was the Socrates Agent App; its North Fleet name is **North Fleet**, and its login is `north-fleet[bot]`. Tom renames the app in GitHub settings; until then its old login still works. Code that recognises the agent bot accepts both `socrates-agent[bot]` and `north-fleet[bot]`, so comments, pushes and notifications the old login wrote stay ours.
- **History.** Dated decision records, incident reports and fixtures keep the names they were written with. They are evidence of what was decided or observed that day, not current vocabulary. The vocabulary guard refuses a new use of either name outside those records.
- **What did not change.** The four layers remain model, harness, agent and fleet; North is still the harness, and a model is never called an agent.
