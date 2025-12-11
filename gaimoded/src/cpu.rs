use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const SCALING_GOV_POLICY_PATH: &'static str =
    "/sys/devices/system/cpu/cpufreq/policy*/scaling_governor";
pub const PERF_GOV: &'static str = "performance";
#[allow(dead_code)]
pub const SCHEDUTIL_GOV: &'static str = "schedutil";
#[allow(dead_code)]
pub const POWERSAVE_GOV: &'static str = "powersave";

pub fn is_gov_available(gov: &str) -> anyhow::Result<bool> {
    // NOTE: 1. cpu*/cpufreq is symlink to ../cpufreq/policy*
    // 2. What if cpu0 has performance and cpu1 does not? So perhaps it's worth it to iterate all cpus and check their available governors
    // // TODO: Check all policies (probably)
    let mut file =
        std::fs::File::open("/sys/devices/system/cpu/cpufreq/policy0/scaling_available_governors")?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    Ok(buf.contains(gov))
}

pub fn set_gov_all(gov: &str) -> anyhow::Result<()> {
    // Since one policy can be used by several cores, it's faster to iterate policies
    for entry in glob::glob(SCALING_GOV_POLICY_PATH)? {
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
    for entry in glob::glob(SCALING_GOV_POLICY_PATH)? {
        // println!("Entry: {}", entry?.to_string_lossy());
        let path = entry?;
        let mut file = std::fs::File::open(&path)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        res.push((path, buf));
    }
    Ok(res)
}
