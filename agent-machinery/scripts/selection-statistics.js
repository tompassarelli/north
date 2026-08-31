
function wilson_lower_bound(successes, known) {
  if (((known <= 0) || (successes < 0) || (successes > known))) {
    return 0.0;
  } else {
    const estimate = (successes / known);
    const z = 1.96;
    const z2 = (z * z);
    const denominator = (1.0 + (z2 / known));
    const center = ((estimate + (z2 / (2.0 * known))) / denominator);
    const variance = (((estimate * (1.0 - estimate)) / known) + (z2 / (4.0 * known * known)));
    const radius = ((z * Math.sqrt(variance)) / denominator);
    return Math.max(0.0, (center - radius));
  }
}
export { wilson_lower_bound as "wilson-lower-bound" };

function wilson_upper_bound(successes, known) {
  if (((known <= 0) || (successes < 0) || (successes > known))) {
    return 1.0;
  } else {
    const estimate = (successes / known);
    const z = 1.96;
    const z2 = (z * z);
    const denominator = (1.0 + (z2 / known));
    const center = ((estimate + (z2 / (2.0 * known))) / denominator);
    const variance = (((estimate * (1.0 - estimate)) / known) + (z2 / (4.0 * known * known)));
    const radius = ((z * Math.sqrt(variance)) / denominator);
    return Math.min(1.0, (center + radius));
  }
}
export { wilson_upper_bound as "wilson-upper-bound" };

function expected_cost_per_pass(mean_cost, successes, known) {
  return (((mean_cost < 0.0) || (known <= 0) || (successes <= 0) || (successes > known)) ? -1.0 : (mean_cost / (successes / known)));
}
export { expected_cost_per_pass as "expected-cost-per-pass" };

function exploration_share_allows_p(eligible_runs, exploration_runs, maximum_share) {
  return ((eligible_runs >= 0) && (exploration_runs >= 0) && (maximum_share >= 0.0) && (maximum_share <= 1.0) && (((exploration_runs + 1) / (eligible_runs + 1)) <= maximum_share));
}
export { exploration_share_allows_p as "exploration-share-allows?" };
