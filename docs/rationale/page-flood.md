# Page floods: bound the rate where every page passes, not in each sender

On 2026-11-10 one lander condition reached the coordinator several hundred times in 15 minutes, because the only thing deciding "already paged" was a per-sender mark and the one that failed had no ceiling below it (north#1048).

## What failed

The lander's build-lag alarm kept "already paged" in one file holding one build id. Two lander builds ran at once, each report overwrote the other's entry, and twelve lander processes, one per repository, all reported north's lag on every round. Twenty other call sites messaged the coordinator the same way, each with its own once-logic or none, and the fleet store passed every service message through.

## The method

- **Bound the rate at the choke point.** Every service message to the coordinator goes through the fleet store's send path, so that is where "once" lives: a service names a condition key, the store delivers one message per key per episode and holds the repeats for the digest. A sender with a wrong key or no key is still folded past 3 messages in 10 minutes into one line with the count, recorded as a `page-flood` incident. One test at the store covers every present and future sender.
- **One mechanism, not one per sender.** The senders' own once-marks were deleted in the same change. Two mechanisms for one guarantee drift apart, and the weaker one decides.
- **Key a "once" record by the condition, never by the last value.** A single slot holding the latest value is only a once-record while every writer holds the same value; the test with one writer cannot see that.
- **Only the owner of a condition reports it.** A fleet of identical processes each reporting a shared condition multiplies one fact by the fleet's size.
- **Alarm on incidents, not standing states.** An alarm whose condition is true most of the time trains its reader to ignore it. Measure in time behind, and let the component that owns the remedy (here the lander doctor's link-behind bound) own the condition.
- **When a rate changes, ask what was sized for the old rate.** Promotion went from every 4 hours to every few minutes; the lag limit, the mark and "restart when idle" were all sized for rare promotions.
- **Make the remedy safe to repeat.** The restart that ended the flood left a half-removed scratch worktree and broke after-landing verification, because creating it was not idempotent.
- **Count against the author, not the relay.** A backstop that keys on the sending process folds distinct workers' reports relayed by one service into one line, withholding exactly what the coordinator exists to read. The fold counts a relayed report against the session that wrote it.
- **Read the emitting code before staffing a fix.** Diagnosing from the symptom (twelve senders) produced a remedy that would not have worked; the 30-line function showed the cause in a minute.

## Considered and not chosen

Fixing only the lander's mark would have taken about 20 minutes and left every other sender, and the coordinator, open to the next bug of the same shape.
