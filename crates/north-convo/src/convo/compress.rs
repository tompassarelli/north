use std::io::IsTerminal;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::Console;
use super::archive::{coordinator_ids, compress_one, open_files, restore_one};
use super::args::Args;
use super::discover::{discover, transcript_roots};
use super::env::Env;
use super::failure::Result;
use super::records::Kind;
use super::render::Palette;

const BAND_RATIOS: [(u64, f64); 5] = [(256 << 20, 15.83), (64 << 20, 7.13), (8 << 20, 5.70), (1 << 20, 4.25), (0, 4.00)];

pub(super) fn gib(bytes: u64) -> String {
    format!("{:.2} GiB", bytes as f64 / (1u64 << 30) as f64)
}

struct Plan {
    eligible: Vec<(String, super::discover::Stat)>,
    total: usize,
    index_only: usize,
    skip_open: usize,
    skip_live: usize,
    skip_hot: usize,
    warm: u64,
}

fn plan(env: &Env, args: &Args, roots: &[super::discover::Root], open: &std::collections::HashSet<String>, roster: &std::collections::HashSet<String>) -> Result<Plan> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64());
    let cutoff = now - args.older_than * 3600.0;
    let mut plan = Plan { eligible: Vec::new(), total: 0, index_only: 0, skip_open: 0, skip_live: 0, skip_hot: 0, warm: 0 };
    let _ = env;
    for found in discover(roots)? {
        if found.disk != found.logical {
            continue;
        }
        if found.kind == Kind::Claude {
            plan.index_only += 1;
            continue;
        }
        plan.total += 1;
        let lower = found.logical.to_lowercase();
        if open.contains(&found.logical) || open.contains(&format!("{}.zst", found.logical)) {
            plan.skip_open += 1;
        } else if roster.iter().any(|id| lower.contains(id)) {
            plan.skip_live += 1;
        } else if found.stat.mtime_secs() > cutoff {
            plan.skip_hot += 1;
            plan.warm += found.stat.size;
        } else {
            plan.eligible.push((found.logical, found.stat));
        }
    }
    plan.eligible.sort_by_key(|(_, stat)| std::cmp::Reverse(stat.size));
    if args.max_files != 0 {
        plan.eligible.truncate(args.max_files);
    }
    Ok(plan)
}

fn report(io: &mut Console, plan: &Plan, roots: &[super::discover::Root], args: &Args, pal: &Palette, ebytes: u64) -> Result<()> {
    let names: Vec<&str> = roots.iter().map(|root| root.path.as_str()).collect();
    writeln!(io.out, "corpus     {} uncompressed transcripts under {}{}{}", plan.total, pal.dim, names.join(", "), pal.off)?;
    writeln!(io.out, "never      {} Claude transcripts (index-only)", plan.index_only)?;
    writeln!(
        io.out,
        "eligible   {} files  {}  (untouched for {}h, no open fd, not on the roster)",
        plan.eligible.len(), gib(ebytes), args.older_than
    )?;
    writeln!(
        io.out,
        "held back  {} still warm ({}) · {} open by a process · {} named by a coordinator lane",
        plan.skip_hot, gib(plan.warm), plan.skip_open, plan.skip_live
    )?;
    Ok(())
}

fn report_hidden(io: &mut Console, unseen: &[String], others: usize) -> Result<()> {
    if !unseen.is_empty() {
        let mut names = unseen.to_vec();
        names.sort();
        names.dedup();
        let more = if names.len() > 6 { format!(", +{}", names.len() - 6) } else { String::new() };
        writeln!(io.out, "unseen     {} of this user's processes hide their open files ({}{more})", unseen.len(), names[..names.len().min(6)].join(", "))?;
    }
    if others != 0 {
        writeln!(io.out, "           {others} more belong to other users")?;
    }
    Ok(())
}

pub(super) fn compress(env: &Env, io: &mut Console, args: &Args, pal: &Palette) -> Result<u8> {
    let roots = transcript_roots(env)?;
    let open = open_files();
    let roster = coordinator_ids(env)?;
    let plan = plan(env, args, &roots, &open.paths, &roster)?;
    let ebytes: u64 = plan.eligible.iter().map(|(_, stat)| stat.size).sum();
    report(io, &plan, &roots, args, pal, ebytes)?;
    report_hidden(io, &open.unseen, open.others)?;
    if plan.eligible.is_empty() {
        return Ok(0);
    }
    if args.dry_run {
        let after: f64 = plan.eligible.iter().map(|(_, stat)| stat.size as f64 / BAND_RATIOS.iter().find(|(floor, _)| stat.size >= *floor).map_or(4.0, |(_, ratio)| *ratio)).sum();
        writeln!(
            io.out,
            "projected  {} → {}  saving {} ({:.2}x, from measured band ratios)",
            gib(ebytes), gib(after as u64), gib(ebytes.saturating_sub(after as u64)), ebytes as f64 / after
        )?;
        return Ok(0);
    }
    sweep(env, io, args, pal, &plan)?;
    Ok(0)
}

fn sweep(env: &Env, io: &mut Console, args: &Args, pal: &Palette, plan: &Plan) -> Result<()> {
    let started = Instant::now();
    let (mut done, mut failed, mut was, mut after) = (0, 0, 0u64, 0u64);
    let progress = !args.quiet && std::io::stderr().is_terminal();
    for (index, (path, stat)) in plan.eligible.iter().enumerate() {
        match compress_one(env, path, stat) {
            Ok(written) => {
                after += written;
                was += stat.size;
                done += 1;
            }
            Err(error @ (super::failure::Failure::Unsafe(_) | super::failure::Failure::Other(_))) => {
                failed += 1;
                writeln!(io.err, "convo: left alone: {path}: {}", error.to_string().trim_start_matches("refusing to sweep: "))?;
            }
            Err(error) => return Err(error),
        }
        if progress {
            eprint!("\r\x1b[Kcompressing {}/{}  {} written", index + 1, plan.eligible.len(), gib(after));
        }
    }
    if progress {
        eprint!("\r\x1b[K");
    }
    let ratio = if after > 0 { format!(" ({:.2}x)", was as f64 / after as f64) } else { String::new() };
    writeln!(io.out, "compressed {done} files  {} → {}  saving {}{ratio}  in {:.0}s", gib(was), gib(after), gib(was.saturating_sub(after)), started.elapsed().as_secs_f64())?;
    if failed > 0 {
        writeln!(io.out, "{}left alone {failed} file(s); see above{}", pal.yellow, pal.off)?;
    }
    Ok(())
}

pub(super) fn restore(env: &Env, io: &mut Console, args: &Args) -> Result<u8> {
    let mut code = 0;
    for name in &args.query {
        match restore_one(env, name, io.err)? {
            Some(line) => writeln!(io.out, "{line}")?,
            None => code = 1,
        }
    }
    Ok(code)
}
