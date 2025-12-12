use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const SCALING_AV_GOV_POLICY_PATH_BLOB: &'static str =
    "/sys/devices/system/cpu/cpufreq/policy*/scaling_available_governors";
pub const SCALING_GOV_POLICY_PATH_GLOB: &'static str =
    "/sys/devices/system/cpu/cpufreq/policy*/scaling_governor";
pub const PERF_GOV: &'static str = "performance";
#[allow(dead_code)]
pub const SCHEDUTIL_GOV: &'static str = "schedutil";
#[allow(dead_code)]
pub const POWERSAVE_GOV: &'static str = "powersave";

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
        // println!("Entry: {}", entry?.to_string_lossy());
        let path = entry?;
        let mut file = std::fs::File::open(&path)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        res.push((path, buf));
    }
    Ok(res)
}
