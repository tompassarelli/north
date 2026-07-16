import {
  addProviderAccount,
  listProviderAccounts,
  loginProviderAccount,
  requireProviderAccount,
  statusProviderAccount,
  type AccountAuthState,
  type ProviderAccount,
} from "./accounts";

const USAGE = `usage: north account <command>

  north account add <safe-id> <anthropic|openai>
  north account login <id>
  north account status [id]
  north account list [--verbose]   grouped accounts + live login state

Options:
  --verbose  include provider, profile, and storage root diagnostics`;

const ACCOUNT_GROUPS = [
  { provider: "anthropic", label: "Claude / Anthropic" },
  { provider: "openai", label: "Codex / OpenAI" },
] as const;

function authLabel(state: AccountAuthState): string {
  switch (state) {
    case "logged-in": return "logged in";
    case "not-logged-in": return "not logged in";
    case "unavailable": return "CLI unavailable";
    case "error": return "auth check failed";
  }
}

function accountStates(accounts: ProviderAccount[]): Map<string, AccountAuthState> {
  return new Map(accounts.map((account) => [account.id, statusProviderAccount(account)]));
}

function printAccountList(
  accounts: ProviderAccount[],
  verbose: boolean,
  states = accountStates(accounts),
): void {
  let firstGroup = true;
  for (const group of ACCOUNT_GROUPS) {
    const grouped = accounts.filter((account) => account.provider === group.provider);
    if (!grouped.length) continue;
    if (!firstGroup) console.log();
    firstGroup = false;
    console.log(group.label);
    const width = Math.max(...grouped.map((account) => account.id.length));
    for (const account of grouped) {
      console.log(`  ${account.id.padEnd(width)}  ${authLabel(states.get(account.id)!)}`);
      if (verbose) {
        console.log(`    provider: ${account.provider}`);
        console.log(`    profile:  ${account.profile}`);
        console.log(`    root:     ${account.root}`);
      }
    }
  }
}

export async function runAccountCli(args: string[]): Promise<number> {
  const [command, ...rest] = args;
  try {
    switch (command) {
      case "add": {
        if (rest.length !== 2) throw new Error(USAGE);
        const account = await addProviderAccount(rest[0], rest[1]);
        console.log(`added isolated ${account.provider} account ${account.id}`);
        console.log(`root ${account.root}`);
        return 0;
      }
      case "login": {
        if (rest.length !== 1) throw new Error(USAGE);
        const account = requireProviderAccount(rest[0]);
        const status = loginProviderAccount(account);
        if (status === 0) console.log(`login complete for ${account.id}`);
        else if (status === 127) console.error(`${account.provider} CLI is not installed`);
        else console.error(`login failed for ${account.id}`);
        return status;
      }
      case "status": {
        if (rest.length > 1) throw new Error(USAGE);
        const accounts = rest.length ? [requireProviderAccount(rest[0])] : listProviderAccounts();
        if (!accounts.length) {
          console.log("no isolated accounts configured");
          return 0;
        }
        const states = accountStates(accounts);
        printAccountList(accounts, false, states);
        return accounts.every((account) => states.get(account.id) === "logged-in") ? 0 : 1;
      }
      case "list": {
        const verbose = rest.length === 1 && rest[0] === "--verbose";
        if (rest.length && !verbose) throw new Error(USAGE);
        const accounts = listProviderAccounts();
        if (!accounts.length) {
          console.log("no isolated accounts configured");
          return 0;
        }
        printAccountList(accounts, verbose);
        return 0;
      }
      case "help":
      case "--help":
      case "-h":
        console.log(USAGE);
        return 0;
      default:
        throw new Error(USAGE);
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 2;
  }
}

if (import.meta.main) process.exit(await runAccountCli(process.argv.slice(2)));
