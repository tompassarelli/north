#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bun test "$here/main-red.test.ts"
bun --eval "
import { issueBody } from '$here/main-red.ts';
const created = issueBody(undefined, '<!-- main-red:1:1 -->', 'first failure');
const appended = issueBody(created, '<!-- main-red:2:1 -->', 'second failure');
const boxes = (text: string) => text.match(/^- \[ \] .*green/gm)?.length;
if (!created.includes('## Done when') || boxes(created) !== 1) throw new Error('new issue lacks its one unticked Done-when box');
if (!appended.startsWith(created) || boxes(appended) !== 1 || !appended.includes('second failure')) throw new Error('appending dropped or duplicated the box');
"
