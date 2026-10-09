# Observation tools

Choose the cheapest existing observation that exposes the good/bad divergence.

- Normalize timestamps, PIDs and addresses before `diff -u` of logs.
- Trace system calls with `strace -f -tt -o run.log` filtered to `file`, `process` or `network`; compare opened paths and `ENOENT`/`EACCES`.
- Trace library calls with `ltrace` or loading with `LD_DEBUG=libs`.
- Trace Wine with `WINEDEBUG=+file,+module,+loaddll` and restrict `+relay` through `RelayInclude`; compare prefixes, DLL overrides and drive mappings.
- Use Process Monitor for native Windows file/process traces.
- Inspect network and IPC with `ss -tupn`, `tcpdump`, devtools or a logging proxy.
- Inspect process state through `/proc/<pid>/{cmdline,environ,cwd,fd,maps}`, `lsof`, `gdb` or core dumps.
- Inspect authorized binaries with `strings`, disassemblers, Frida or `LD_PRELOAD` hooks.
- Prefer documented status/control APIs and structured state over screenshots or OCR.
- Bisect commits, inputs or configuration when the candidate differences are numerous.
- End symptom research after a known cause, workaround or diagnostic technique, or a handful of unsuccessful queries.
- Report a workaround as a mitigation with the underlying cause recorded.
