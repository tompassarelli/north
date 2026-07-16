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
  north account list`;

function printAccount(account: ProviderAccount): void {
  console.log(`${account.id}\t${account.provider}\t${account.profile}\t${account.root}`);
}

function printStatus(account: ProviderAccount, state: AccountAuthState): void {
  console.log(`${account.id}\t${account.provider}\t${state}`);
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
        let ready = true;
        for (const account of accounts) {
          const state = statusProviderAccount(account);
          printStatus(account, state);
          if (state !== "logged-in") ready = false;
        }
        return ready ? 0 : 1;
      }
      case "list": {
        if (rest.length) throw new Error(USAGE);
        const accounts = listProviderAccounts();
        if (!accounts.length) {
          console.log("no isolated accounts configured");
          return 0;
        }
        for (const account of accounts) printAccount(account);
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
