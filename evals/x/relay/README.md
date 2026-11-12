# Eval X: the shell relay

Both harnesses and Claude Code stay on the host; only the shell tool's commands reach the task's container. The
relay is the program Claude Code is given as its shell.

- `probe-shell`: a shell that appends every call it gets (arguments, working directory, the names of its
  environment variables) to the file `EVAL_X_SHELL_LOG` names, then runs the real shell. It shows how a Claude
  Code version calls its shell, for hooks as well as for the shell tool.
- `x3 TOKEN-FILE [MODEL]`: one `claude -p` session on the subscription token in that file, with `probe-shell` as
  its shell and one hook configured, asked for a plain command, a `cd`, a `pwd` in the next call, a background
  command and a command that passes its time limit. It prints the reply and every call under `/tmp/eval-x/x3`.
  It needs a login, so it is started outside a worker session.
