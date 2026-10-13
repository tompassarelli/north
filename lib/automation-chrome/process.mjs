import { readFile } from 'node:fs/promises';

export async function processIdentity(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 1) throw new Error('Invalid process');
  const stat = await readFile(`/proc/${pid}/stat`, 'utf8');
  return stat.slice(stat.lastIndexOf(')') + 2).split(' ')[19];
}
