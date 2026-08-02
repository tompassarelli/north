import { Northd } from "./host";

const northd = new Northd();
await northd.listen();

let closing = false;
const close = () => {
  if (closing) return;
  closing = true;
  void northd.close().finally(() => process.exit(0));
};
process.once("SIGINT", close);
process.once("SIGTERM", close);
