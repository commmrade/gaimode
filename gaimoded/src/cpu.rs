use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const SCALING_AV_GOV_POLICY_PATH_BLOB: &'static str =
    "/sys/devices/system/cpu/cpufreq/policy*/scaling_available_governors";
pub const SCALING_GOV_POLICY_PATH_GLOB: &'static str =
    "/sys/devices/system/cpu/cpufreq/policy*/scaling_governor";
pub const PERF_GOV: &'static str = "performance";

pub fn is_gov_available(gov: &str) -> anyhow::Result<bool> {
    // NOTE: 1. cpu*/cpufreq is symlink to ../cpufreq/policy*
    for entry in glob::glob(SCALING_AV_GOV_POLICY_PATH_BLOB)? {
        let mut file = std::fs::File::open(entry?)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;

        if buf.contains(gov) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn set_gov_all(gov: &str) -> anyhow::Result<()> {
    // Since one policy can be used by several cores, it's faster to iterate policies
    for entry in glob::glob(SCALING_GOV_POLICY_PATH_GLOB)? {
        let mut file = std::fs::OpenOptions::new().write(true).open(entry?)?;
        file.write(gov.as_bytes())?;
    }
    Ok(())
}

pub fn set_gov(path: &Path, gov: &str) -> anyhow::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.write(gov.as_bytes())?;
    Ok(())
}

pub fn get_govs() -> anyhow::Result<Vec<(PathBuf, String)>> {
    let mut res = Vec::new();
    for entry in glob::glob(SCALING_GOV_POLICY_PATH_GLOB)? {
        let path = entry?;
        let mut file = std::fs::File::open(&path)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        res.push((path, buf));
    }
    Ok(res)
}

// Number of hardware threads
pub fn cpus_num() -> anyhow::Result<i64> {
    unsafe {
        let ret = libc::sysconf(libc::_SC_NPROCESSORS_CONF);
        if ret < 0 {
            return Err(anyhow::anyhow!("Could not fetch number of CPUS online"));
        }
        Ok(ret)
    }
}

// Returns a CPU load percentage (index is cpu number)
pub fn cpus_load() -> anyhow::Result<Vec<(usize, f32)>> {
    let mut res = Vec::new();
    let cpus_n = cpus_num()?;

    let mut cpu_stat_start = String::new();
    let mut cpu_stat_end = String::new();

    {
        let mut file = std::fs::File::open("/proc/stat")?;
        file.read_to_string(&mut cpu_stat_start)?;
    }
    std::thread::sleep(Duration::from_secs(1));
    {
        let mut file = std::fs::File::open("/proc/stat")?;
        file.read_to_string(&mut cpu_stat_end)?;
    }

    let cpu_values_start = &cpu_stat_start
        .split('\n')
        .map(|line| line.split(' ').collect::<Vec<&str>>())
        .collect::<Vec<Vec<&str>>>()[1..cpus_n as usize];
    let cpu_values_end = &cpu_stat_end
        .split('\n')
        .map(|line| line.split(' ').collect::<Vec<&str>>())
        .collect::<Vec<Vec<&str>>>()[1..cpus_n as usize];

    for (idx, (values_start, values_end)) in cpu_values_start
        .iter()
        .zip(cpu_values_end.iter())
        .enumerate()
    {
        if values_start.is_empty() {
            continue;
        }

        // Parse deltas
        let user_d = values_end[1].parse::<u32>()? - values_start[1].parse::<u32>()?;
        let nice_d = values_end[2].parse::<u32>()? - values_start[2].parse::<u32>()?;
        let system_d = values_end[3].parse::<u32>()? - values_start[3].parse::<u32>()?;
        let idle_d = values_end[4].parse::<u32>()? - values_start[4].parse::<u32>()?;
        let iowait_d = values_end[5].parse::<u32>()? - values_start[5].parse::<u32>()?;
        let irq_d = values_end[6].parse::<u32>()? - values_start[6].parse::<u32>()?;
        let softirq_d = values_end[7].parse::<u32>()? - values_start[7].parse::<u32>()?;

        // Total delta = sum of all deltas
        let total_d = user_d + nice_d + system_d + idle_d + iowait_d + irq_d + softirq_d;

        // Idle time
        let idle_total_d = idle_d + iowait_d;

        // CPU load percentage
        let load = if total_d == 0 {
            0.0
        } else {
            ((total_d - idle_total_d) as f32 / total_d as f32) * 100.0f32
        };

        res.push((idx, load));
    }
    Ok(res)
}

pub fn get_aff_mask(pid: nix::unistd::Pid) -> anyhow::Result<libc::cpu_set_t> {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        let ret = libc::sched_getaffinity(
            pid.as_raw(),
            std::mem::size_of::<libc::cpu_set_t>(),
            &mut set as *mut _,
        );
        if ret < 0 {
            return Err(anyhow::anyhow!("Could not get process affinity"));
        }
        Ok(set)
    }
}

pub fn set_aff_mask(pid: nix::unistd::Pid, mask: libc::cpu_set_t) -> anyhow::Result<()> {
    unsafe {
        let ret = libc::sched_setaffinity(
            pid.as_raw(),
            std::mem::size_of::<libc::cpu_set_t>(),
            &mask as *const _,
        );
        if ret < 0 {
            return Err(anyhow::anyhow!("Could not change process affinity"));
        }
        Ok(())
    }
}

pub fn pin_process(pid: nix::unistd::Pid, cpu: usize) -> anyhow::Result<()> {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(cpu, &mut set);
        let ret = libc::sched_setaffinity(
            pid.as_raw(),
            std::mem::size_of::<libc::cpu_set_t>(),
            &set as *const _,
        );
        if ret < 0 {
            return Err(anyhow::anyhow!("Could not change process affinity"));
        }
        Ok(())
    }
}

pub fn pin_process_excluding(pid: nix::unistd::Pid, cpu_exclude: usize) -> anyhow::Result<()> {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();

        let cpus_n = cpus_num()?;
        for i in 0..cpus_n {
            libc::CPU_SET(i as usize, &mut set);
        }
        libc::CPU_CLR(cpu_exclude, &mut set);

        // libc::CPU_SET(cpu, &mut set);
        let ret = libc::sched_setaffinity(
            pid.as_raw(),
            std::mem::size_of::<libc::cpu_set_t>(),
            &set as *const _,
        );
        if ret < 0 {
            return Err(anyhow::anyhow!("Could not change process affinity"));
        }
    }
    Ok(())
}

pub fn cpu_core_id(cpu: usize) -> anyhow::Result<usize> {
    let path = format!("/sys/devices/system/cpu/cpu{}/topology/core_id", cpu);
    let mut file = std::fs::File::open(&path)?;
    let mut str = String::new();
    file.read_to_string(&mut str)?;
    let num = str.trim().parse::<usize>()?;
    Ok(num)
}
