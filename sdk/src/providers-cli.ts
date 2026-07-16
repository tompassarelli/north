import { probeAnthropic, probeOpenAI, resourcePolicyFromEnv, selectProviderFromAvailability } from "./provider-routing";
import { refreshCodexEntitlementIfStale } from "./codex-entitlement";
import { northSourceIdentity } from "./providers/source-identity";

function sourceIdentity(): string {
  const root = new URL("../..", import.meta.url).pathname;
  return northSourceIdentity(root);
}

try {
  await refreshCodexEntitlementIfStale();
  const policy = resourcePolicyFromEnv();
  const targets = policy.targets?.length ? policy.targets : [
    { id: "anthropic", provider: "anthropic" as const, authMode: "ambient" as const },
    { id: "openai", provider: "openai" as const, authMode: "ambient" as const },
  ];
  const availability = targets.map((target) => ({
    ...(target.provider === "anthropic" ? probeAnthropic(target) : probeOpenAI(target)),
    targetId: target.id,
  }));
  console.log(`source     ${sourceIdentity()}`);
  for (const p of availability) {
    const headroom = policy.targetPressures?.[p.targetId!] ?? policy.pressures[p.provider] ?? "unknown";
    const routing = p.reason === "disabled" ? "disabled" : p.available ? "eligible" : "unavailable";
    console.log(`${p.targetId!.padEnd(18)} provider=${p.provider.padEnd(9)} installed=${p.installed ? "yes" : "no"}  authenticated=${p.authenticated ? "yes" : "no"}  headroom=${headroom}  routing=${routing}${p.detail ? `  ${p.detail}` : ""}`);
  }
  try {
    const d = selectProviderFromAvailability("auto", availability, policy);
    console.log(`auto       ${d.target} (${d.provider})  ${d.reason}`);
  } catch (error: any) {
    console.log(`auto       unavailable  ${error?.message ?? error}`);
  }
} catch (err: any) {
  console.error(err?.message ?? err);
  process.exit(1);
}
