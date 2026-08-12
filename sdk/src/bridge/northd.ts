import { Northd } from "./host";

let closing = false;
const close = () => {
  if (closing) return;
  closing = true;
  void northd.close().then(() => process.exit(0), () => process.exit(1));
};
const northd = new Northd({ onRetire: close });
await northd.listen();

process.once("SIGINT", close);
process.once("SIGTERM", close);
