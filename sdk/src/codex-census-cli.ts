import { listProviderAccounts, type AccountContext } from "./accounts";

/** Config identifies subscription accounts; Store facts remain session authority. */
export function configuredCodexSubscriptionAccounts(context: AccountContext = {}): string[] {
  return listProviderAccounts(context)
    .filter((account) => account.provider === "openai")
    .map((account) => account.id)
    .sort();
}

if (import.meta.main) console.log(JSON.stringify(configuredCodexSubscriptionAccounts()));
