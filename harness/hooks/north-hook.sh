#!/usr/bin/env bash
here="${0%/*}"
if [ -r "$here/lib/authoring-killswitch.sh" ]; then
  . "$here/lib/authoring-killswitch.sh"
else
  . "$here/../lib/authoring-killswitch.sh"
fi
authoring_guards_off && exit 0
exec north hook
