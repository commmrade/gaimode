use serde::Deserialize;

use crate::{cpu, io, scheduler};

#[derive(Deserialize)]
pub struct CpuAffinity {
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct CpuGovernor {
    pub enabled: bool,
    pub optimized_type: String,
}

#[derive(Deserialize)]
pub struct Niceness {
    pub enabled: bool,
    pub optimized_value: i32,
    pub default_value: i32,
}

#[derive(Deserialize)]
pub struct IoNiceness {
    pub enabled: bool,
    pub optimized_value: i32,
    pub default_value: i32,
}

#[derive(Deserialize)]
pub struct Settings {
    pub cpu_affinity: CpuAffinity,
    pub cpu_governor: CpuGovernor,
    pub niceness: Niceness,
    pub ioniceness: IoNiceness,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cpu_affinity: CpuAffinity { enabled: true },
            cpu_governor: CpuGovernor {
                enabled: true,
                optimized_type: cpu::PERF_GOV.to_owned(),
            },
            niceness: Niceness {
                enabled: true,
                optimized_value: scheduler::OPTIMIZED_NICE_VALUE,
                default_value: scheduler::DEFAULT_NICE_VALUE,
            },
            ioniceness: IoNiceness {
                enabled: true,
                optimized_value: io::OPTIMIZED_IO_NICE_VALUE,
                default_value: io::DEFAULT_IO_NICE_VALUE,
            },
        }
    }
}

impl Settings {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let cfg = config::Config::builder()
            .add_source(config::File::with_name(path).required(false))
            .build()?;

        let s = cfg.try_deserialize::<Self>()?;
        Ok(s)
    }
}

pub fn get_cfg() -> anyhow::Result<Settings> {
    let mut config_path = std::env::home_dir().ok_or(anyhow::anyhow!("No home dir set"))?;
    config_path.push(".config/gaimode/settings.toml");

    Ok(Settings::from_file(&config_path.to_str().ok_or(
        anyhow::anyhow!("Could not convert path to str"),
    )?)?)
}
