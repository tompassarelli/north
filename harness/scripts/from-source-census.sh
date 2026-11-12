#!/usr/bin/env bash
# Census of the workspace crates against what a from-source Rust runner needs (docs/rationale/compile-reuse.md,
# "Runnable from source"). Reads Cargo.toml, Cargo.lock, build scripts and sources; builds nothing.
# Usage: from-source-census.sh [REPO] [DAY]   REPO defaults to ., DAY (YYYY-MM-DD) to yesterday.
set -uo pipefail
repo=$(cd "${1:-.}" && pwd); day=${2:-$(date -d yesterday +%F)}; cd "$repo"
out=$(mktemp -d); trap 'rm -rf "$out"' EXIT
cargo metadata --offline --locked --format-version 1 >"$out/meta.json" 2>/dev/null
north code-health crate-lines | awk '{print $2"\t"$3}' >"$out/lines"
# member<TAB>kind<TAB>label<TAB>dir for every build-script (c, links, build) and proc-macro (pm) package in its host build graph (cargo tree, not metadata: the
# metadata graph lists optional dependencies no feature turns on)
jq -r '.packages[]|"\(.name)@\(.version)\t\([.targets[].kind[]]|if index("proc-macro") then "pm" elif index("custom-build") then "b" else "-" end)\t\(.links//"-")\t\(.manifest_path|sub("/Cargo.toml$";""))"' "$out/meta.json" | sort >"$out/info"
cargo tree --offline --locked --workspace -e normal,build --no-dedupe --prefix depth --format '{p}' 2>/dev/null | awk '
  {match($0,/^[0-9]+/); d=substr($0,1,RLENGTH)+0; split(substr($0,RLENGTH+1),a," "); id=a[1]"@"substr(a[2],2); st[d]=id; if(d==0)m=a[1]
   if(d>0&&a[1]~/^(cc|cmake|bindgen|pkg-config|vcpkg|nasm-rs)$/)cb[m,st[d-1]]=1; seen[m,id]=1}
  END{for(k in seen){split(k,p,SUBSEP); print p[1]"\t"p[2]"\t"((p[1],p[2]) in cb?"c":"-")}}' | sort >"$out/tree"
awk -F'\t' 'NR==FNR{k[$1]=$2; l[$1]=$3; d[$1]=$4; next} k[$2]!="-"{kind=(k[$2]=="pm")?"pm":($3=="c")?"c":(l[$2]!="-")?"links":"build"; print $1"\t"kind"\t"$2"\t"d[$2]}' "$out/info" "$out/tree" | sort -u >"$out/closure"
# the members' own directories and the external crates each declares directly
jq -r '.packages[]|select(.source==null)|"\(.name)\t\(.manifest_path|sub("/Cargo.toml$";""))\t\([.dependencies[]|select(.kind!="dev" and .path==null)|.name]|unique|join(","))\t\([.dependencies[]|select(.kind=="dev" and .path!=null)|.name]|unique|join(" "))"' "$out/meta.json" | sort >"$out/members"
while IFS=$'\t' read -r kind label dir; do # build scripts that read DEP_ links variables, proc-macro names
  [ "$kind" = build ] && rg -q 'DEP_' "$dir/build.rs" 2>/dev/null && kind=links
  names=$(rg -U --no-filename -o 'proc_macro_derive\(\s*(\w+)|#\[proc_macro\]\s*pub fn (\w+)' -r '$1$2' "$dir" 2>/dev/null | tr '\n' ' ')
  printf '%s\t%s\t%s\n' "$kind" "$label" "$names"
done < <(cut -f2- "$out/closure" | sort -u) >"$out/pkgs"
testscan='BEGIN{OFS="\t"} FNR==1{flush()} {if(/#\[cfg\(test\)\]/&&!r)r=FNR; if(FILENAME~/(^|\/)tests\/|_tests\.rs$|\/tests\.rs$/&&!r)r=1;
  if(r&&FNR>=r){if(/#\[(tokio::)?test/)t++; if(/CARGO_BIN_EXE|bin_exe!/)b=1; if(/Command::new|\.spawn\(|tokio::process/)o=1}}
  function flush(){u+=(b||o)?0:t; pb+=b?t:0; po+=(!b&&o)?t:0; t=b=o=0; r=0} END{flush(); print u+0,pb+0,po+0}'
{ printf 'crate\tdir\tnontest_lines\tbuild_rs\tnative_c\ttest_native_via\tbuild_scripts\tproc_macro_crates\tmacro_name_clashes\tfeature\tunsafe\textern\tasm\thost_interfaces\tsys_sites\tnative_calls\ttests_in_process\ttests_own_binary\ttests_other_process\town_macros\tdirect_external\tbase_sql_api\tgroup\n'
while IFS=$'\t' read -r crate path direct devs; do
  [ "$crate" = north ] && dirs="src tests build.rs" || dirs="src tests"; cd "$path"
  sc() { rg --no-messages -c "$1" --glob '*.rs' $dirs | awk -F: '{s+=$NF} END{print s+0}'; }
  nt() { rg --no-messages -U -c "$1" --glob '*.rs' --glob '!tests/**' --glob '!*_tests.rs' --glob '!tests.rs' $dirs | awk -F: '{s+=$NF} END{print s+0}'; }
  pick() { awk -F'\t' -v c="$crate" -v k="$1" '$1==c&&$2==k{print $3}' "$out/closure" | sort -u | paste -sd,; }
  c=$(pick c); l=$(for d in $devs; do awk -F"\t" -v d="$d" '$1==d&&$2=="c"{f=1} END{exit !f}' "$out/closure" && echo "$d"; done | paste -sd,); b=$(awk -F'\t' -v c="$crate" '$1==c&&$2!="pm"' "$out/closure" | wc -l)
  pm=$(awk -F'\t' -v c="$crate" '$1==c&&$2=="pm"{print $3}' "$out/closure" | sort -u)
  clash=$(for p in $pm; do awk -F'\t' -v p="$p" '$2==p{print $3}' "$out/pkgs" | tr ' ' '\n' | grep . | sort -u | sed "s/\$/ $p/"; done | sort | awk '{n[$1]++; p[$1]=p[$1]"+"$2} END{for(k in n)if(n[k]>1)printf "%s%s ",k,p[k]}')
  abi=$(for t in 'ptrace::' 'landlock::' 'seccompiler::' 'rustix::pty::' 'rustix::mount::' pidfd_open CLONE_NEW current_exe; do [ "$(nt "$t")" -gt 0 ] && echo "${t#rustix::}"; done | sed "s/::$//" | paste -sd,)
  files=$(find $dirs -name '*.rs' 2>/dev/null | sort)
  read -r tu tb to < <([ -n "$files" ] && awk "$testscan" $files || echo "0 0 0")
  own=$( { rg -q "derive\([^)]*(Serialize|Deserialize)" --glob "*.rs" $dirs 2>/dev/null && echo serde; rg -q "#\[tokio::(main|test)" --glob "*.rs" $dirs 2>/dev/null && echo tokio; } | paste -sd,)
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$crate" "$(realpath --relative-to="$repo" "$path")" \
    "$(awk -F'\t' -v c="$crate" '$1==c{print $2}' "$out/lines")" "$([ -f build.rs ] && echo y || echo n)" "${c:--}" "${l:--}" "$b" "$(echo "$pm" | grep -c .)" "${clash:--}" \
    "$(sc '#!?\[feature')" "$(sc 'unsafe\s+(fn|impl|trait|extern)|unsafe\s*\{')" "$(sc 'extern\s+"|extern\s*\{')" "$(sc '\b(asm|global_asm|naked_asm)!')" "${abi:--}" \
    "$(nt '\b(rustix|libc|nix|landlock|seccompiler)::')" "$(nt '\b(rusqlite|ring)::')" "$tu" "$tb" "$to" "${own:--}" "${direct:--}" "$(nt "lifecycle::(Store|\{[^}]*\bStore\b)|platform::sqlite|sqlite::(rows|\{)|usage_table")"
  cd "$repo"
done <"$out/members"; } | awk -F'\t' 'NR==1{print; next} {g=($14~/ptrace|landlock|seccomp|CLONE|mount/||$1=="north")?"3 native":($5!="-"||$6!="-"||$9!="-")?(($16>0)?"2b one move (own SQL)":($22>0)?"2c one move (north-base SQL API)":"2a one move (north-base)"):"1 as is"; print $0"\t"g}' >"$out/table"
md='NR==1{print "| Crate | Non-test lines | build.rs (own / in graph) | Native C reached | Proc-macro crates in graph | Own macros | Sys crates declared | Host interfaces | unsafe, extern, asm, feature | Tests: in-process, own binary, other process | Group |"; print "|---|---|---|---|---|---|---|---|---|---|---|"; next}
  {s=""; n=split($21,d,","); for(i=1;i<=n;i++)if(d[i]~/^(rusqlite|ring|rustix|tokio|libc|nix|landlock|seccompiler|crossterm)$/)s=s (s?", ":"") d[i]; nat=$5; gsub(/libsqlite3-sys@[0-9.]+/,"SQLite",nat); gsub(/ring@[0-9.]+/,"ring",nat); gsub(/,/,", ",nat)
   if($16>0)nat=nat " (calls: " $16 ")"; print "| " $1 " | " $3 " | " $4 " / " $7 " | " (nat=="-"?($6=="-"?"none":"none (tests via "$6")"):nat) " | " $8 " | " $20 " | " (s==""?"-":s) " | " $14 " | " $11 ", " $12 ", " $13 ", " $10 " | " $17 ", " $18 ", " $19 " | " $23 " |"}'
if [ -n "${MD:-}" ]; then { head -1 "$out/table"; tail -n +2 "$out/table" | sort -t$'\t' -k23,23 -k1,1; } | awk -F'\t' "$md"; else cat "$out/table"; fi
printf '\ngroup\tcrates\tlines\ttests_in_process\ttests_own_binary\ttests_other_process\n'
awk -F'\t' 'function add(k){c[k]++; l[k]+=$3; u[k]+=$17; b[k]+=$18; o[k]+=$19}
  NR>1{add($23); add("all"); if($23~/^(1|2a)/||$1=="north-base")add("1+2a+north-base")} END{for(k in c)printf "%s\t%d\t%d\t%d\t%d\t%d\n",k,c[k],l[k],u[k],b[k],o[k]}' "$out/table" | sort
printf '\nday %s\tgroup\ttests_passed\tseconds\n' "$day"
for f in $(find ~/.local/state/north/land-shadow -name '*.log' -newermt "$day" ! -newermt "$day +1 day"); do rg -o 'PASS \[\s*([0-9.]+)s\] \(\s*\d+/\d+\) (\S+)' -r '$2 $1' "$f"; done \
  | awk -F'\t' 'function add(k){n[k]++; s[k]+=a[2]} NR==FNR{if(FNR>1){g[$1]=$23; p[$1]=($23~/^(1|2a)/||$1=="north-base")}; next}
    {split($0,a," "); id=a[1]; sub(/::.*/,"",id); if(!(id in g))next; add(g[id]); add("all"); if(p[id])add("1+2a+north-base")} END{for(k in n)printf "\t%s\t%d\t%.0f\n",k,n[k],s[k]}' "$out/table" - | sort
printf '\nbinary_spawn_sites (CARGO_BIN_EXE, bin_exe!)\tcrate\n'
rg -c 'CARGO_BIN_EXE|bin_exe!' --glob '*.rs' . | awk -F: '{split($1,p,"/"); k=($1~/^\.\/crates\//)?p[3]:"north"; s[k]+=$2} END{for(k in s)printf "%d\t%s\n",s[k],k}' | sort -rn
