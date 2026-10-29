#!/usr/bin/env -S bun --no-install
// @bun
import{constants as o}from"os";import{join as n}from"path";var i=process.env.AGENT_CAPACITY_NORTH??n(process.env.HOME??"",".local/state/north/current/north"),s=Bun.spawn([i,"capacity","host",...Bun.argv.slice(2)],{stdin:"inherit",stdout:"inherit",stderr:"inherit"});for(let t of["SIGINT","SIGTERM","SIGHUP"])process.on(t,()=>s.kill(t));var r=await s.exited;process.exitCode=s.signalCode?128+o.signals[s.signalCode]:r;
