use super::gpu::pmon_busy_fractions;

const PMON: &str = "\
# gpu         pid   type     sm    mem    enc    dec    jpg    ofa    command 
# Idx           #    C/G      %      %      %      %      %      %    name 
    0       4242     C     37     12      -      -      -      -    wisp-render    
    0       4242     G      5      1      -      -      -      -    wisp-render    
    0       5151     C      -      -      -      -      -      -    idle-holder    
    0       6060     G      2      0     60      -      -      -    ffmpeg         
    0       7070     C     90     40      -      -      -      -    other-lease    
";

fn pids(list: &[&str]) -> Vec<String> {
    list.iter().map(|pid| (*pid).to_owned()).collect()
}

// KEEP(contract): parses nvidia-smi pmon output, busiest engine per leased pid
#[test]
fn pmon_counts_each_leased_process_at_its_busiest_engine_and_ignores_memory_bandwidth() {
    let busy = pmon_busy_fractions(PMON, &pids(&["4242", "5151", "6060"]));
    assert!((busy - (0.37 + 0.60)).abs() < 1e-9, "{busy}");
}
