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

#[test]
fn pmon_counts_each_leased_process_at_its_busiest_engine_and_ignores_memory_bandwidth() {
    let busy = pmon_busy_fractions(PMON, &pids(&["4242", "5151", "6060"]));
    assert!((busy - (0.37 + 0.60)).abs() < 1e-9, "{busy}");
}

#[test]
fn pmon_ignores_processes_outside_the_lease_and_headers_and_an_idle_gpu() {
    assert_eq!(pmon_busy_fractions(PMON, &pids(&["9999"])), 0.0);
    let idle = "# gpu pid type sm mem enc dec jpg ofa command\n# Idx # C/G % % % % % % name\n    0       -     -      -      -      -      -      -      -    -\n";
    assert_eq!(pmon_busy_fractions(idle, &pids(&["4242"])), 0.0);
}

mod recorded_radeon {
    use super::super::gpu::{GpuClient, GpuProcessSample, top_unleased_gpu_process};
    use super::super::policy::gpu_busy_observation;
    use serde_json::{Value, json};

    const SHADER_CGROUP: &str = "/user.slice/user-1000.slice/user@1000.service/agent.slice/run-p970853-i42884880.scope";

    fn shader(client: &str, pid: u64, cgroup: &str, nanoseconds: f64) -> GpuClient {
        GpuClient { client: client.to_owned(), pid, start: "15663073".to_owned(), command: "python3 /tmp/north18-gpu.PLbFtc/gpu-load.py".to_owned(), cgroup: cgroup.to_owned(), nanoseconds }
    }

    fn sample(at: f64, counters: [f64; 3], peer: Option<f64>) -> GpuProcessSample {
        let mut clients: Vec<GpuClient> = counters.iter().map(|&nanoseconds| shader("0000:c1:00.0:10911", 1_111_436, SHADER_CGROUP, nanoseconds)).collect();
        clients.extend(peer.map(|nanoseconds| shader("0000:c1:00.0:10912", 1_111_437, "/leased/child", nanoseconds)));
        GpuProcessSample { at, clients }
    }

    fn before(peer: Option<f64>) -> GpuProcessSample {
        sample(1_791_727_704_584.0, [18_737_788_555.0, 18_737_888_983.0, 18_737_957_632.0], peer)
    }

    fn after(peer: Option<f64>) -> GpuProcessSample {
        sample(1_791_727_724_899.0, [35_485_478_339.0, 35_485_571_223.0, 35_485_636_095.0], peer)
    }

    fn top(before: &GpuProcessSample, after: &GpuProcessSample, leased: &[&str]) -> Value {
        top_unleased_gpu_process(before, after, &leased.iter().map(|group| (*group).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn the_recorded_headless_shader_is_named_with_its_share_of_the_gpu_and_a_leased_or_restarted_one_is_not() {
        let expected = json!({ "pid": 1_111_436, "command": "python3 /tmp/north18-gpu.PLbFtc/gpu-load.py", "gpuBusyPercent": 82.44 });
        assert_eq!(top(&before(None), &after(None), &[]), expected);
        assert_eq!(top(&before(Some(0.0)), &after(Some(20e9)), &["/leased"]), expected);
        assert_eq!(top(&before(None), &after(None), &[SHADER_CGROUP]), Value::Null);
        let mut restarted = after(None);
        for client in &mut restarted.clients {
            client.start = "reused-pid".to_owned();
        }
        assert_eq!(top(&before(None), &restarted, &[]), Value::Null);
    }

    #[test]
    fn the_gpu_counts_as_busy_from_85_percent_and_the_busy_duration_clears_below_it() {
        let since = 1_791_727_689_793.0;
        assert_eq!(gpu_busy_observation(1_791_727_694_434.0, 99.0, Some(since)), (Some(since), 4.641));
        assert_eq!(gpu_busy_observation(since, 85.0, None), (Some(since), 0.0));
        assert_eq!(gpu_busy_observation(1_791_727_724_899.0, 84.0, Some(since)), (None, 0.0));
    }
}
