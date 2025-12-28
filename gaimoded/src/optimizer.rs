use std::{collections::HashMap, path::PathBuf};

use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    cfg, cpu, io, scheduler,
    utils::{self},
};

struct CpuState {
    path: PathBuf,
    governor: String,
}
impl Default for CpuState {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            governor: String::new(),
        }
    }
}

#[allow(dead_code)]
struct ProcessState {
    niceness: Option<i32>,
    ioniceness: Option<i32>,
    aff_mask: Option<libc::cpu_set_t>, // Store main thread affinity mask
}
impl Default for ProcessState {
    fn default() -> Self {
        Self {
            niceness: None,
            ioniceness: None,
            aff_mask: None,
        }
    }
}

struct SystemState {
    cpus_state: Vec<CpuState>,
}

#[allow(dead_code)]
pub struct Optimizer {
    system_state: Option<SystemState>, // optimizer state basically
    processes: HashMap<nix::unistd::Pid, ProcessState>,
    settings: cfg::Settings,
}

impl Optimizer {
    pub fn new(settings: cfg::Settings) -> Self {
        Self {
            system_state: None,
            processes: HashMap::new(),
            settings,
        }
    }

    fn optimize(&mut self) -> anyhow::Result<()> {
        if self.settings.cpu_governor.enabled {
            let is_perf_supported = cpu::is_gov_available(cpu::PERF_GOV)?;
            if !is_perf_supported {
                return Err(anyhow::anyhow!(
                    "Your policies do not support 'Performance' governor"
                ));
            }

            let govs = cpu::get_govs()?;

            let mut cpus_state = Vec::new();
            cpus_state.reserve(govs.len());
            for (path, gov) in govs.into_iter() {
                let mut state = CpuState::default();
                state.governor = gov;
                state.path = path;
                cpus_state.push(state);
            }

            cpu::set_gov_all(cpu::PERF_GOV)?;
            self.system_state = Some(SystemState { cpus_state })
        }
        Ok(())
    }

    fn reset_system(&mut self) -> anyhow::Result<()> {
        if self.settings.cpu_governor.enabled {
            if let Some(state) = self.system_state.as_ref() {
                for cur in &state.cpus_state {
                    // If 1 fails, try to reset all other policies
                    if let Err(why) = cpu::set_gov(&cur.path, &cur.governor) {
                        tracing::error!(
                            "Setting gov for {} failed: {}",
                            cur.path.to_string_lossy(),
                            why
                        );
                    }
                }
            }
        }
        Ok(())
    }
    fn reset_processes(&mut self) -> anyhow::Result<()> {
        for (process, state) in self.processes.drain() {
            reset_process(process, state, &self.settings)?;
        }
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::info!("Resetting all optimizations");
        if let Err(why) = self.reset_processes() {
            tracing::error!("Reset processes failure: {}", why);
        }
        if let Err(why) = self.reset_system() {
            tracing::error!("Reset system failure: {}", why);
        }
        self.system_state = None;
        Ok(())
    }

    fn clear_dead_pids(&mut self) -> bool {
        let mut has_removed = false;
        self.processes.retain(|pid, _| {
            // let res = unsafe { nix::libc::kill(pid.as_raw(), 0) }
            match nix::sys::signal::kill(*pid, None) {
                Ok(_) => return true,                         // process alive
                Err(nix::errno::Errno::EPERM) => return true, // alive, but not enough permissions to send the signal
                Err(_) => {
                    has_removed = true;
                    return false;
                }
            };
        });
        has_removed
    }

    fn add_process(&mut self, pid: nix::unistd::Pid) -> anyhow::Result<()> {
        let mut pstate = ProcessState::default();

        if self.settings.niceness.enabled {
            match scheduler::process_niceness(pid) {
                Ok(nc) => pstate.niceness = Some(nc),
                Err(why) => {
                    tracing::warn!("Getting process niceness failure: {}", why);
                }
            }
        }
        if self.settings.ioniceness.enabled {
            match io::process_io_niceness(pid) {
                Ok(nc) => pstate.ioniceness = Some(nc),
                Err(why) => {
                    tracing::warn!("Getting process ioniceness failure: {}", why);
                }
            }
        }
        if self.settings.cpu_affinity.enabled {
            match cpu::get_aff_mask(pid) {
                Ok(mask) => pstate.aff_mask = Some(mask),
                Err(why) => {
                    tracing::warn!("Getting affinity mask failure: {}", why);
                }
            }
        }

        optimize_process(pid, &self.settings);
        self.processes.insert(pid, pstate);
        Ok(())
    }

    pub async fn process(
        &mut self,
        rx: &mut UnboundedReceiver<utils::Commands>,
    ) -> anyhow::Result<()> {
        if let Ok(command) = rx.try_recv() {
            match command {
                utils::Commands::OptimizeProcess(pid) => {
                    if self.system_state.is_none() {
                        self.optimize()?;
                    }
                    self.add_process(pid)?;
                }
                utils::Commands::ResetProcess(pid) => {
                    if let Some(state) = self.processes.remove(&pid) {
                        reset_process(pid, state, &self.settings)?;
                    }
                }
                utils::Commands::ResetAll => {
                    self.reset()?;
                }
            }
        }

        if let Some(_) = self.system_state.as_ref() {
            self.clear_dead_pids();
            if self.processes.is_empty() {
                // No processes to track, so reset the system state
                self.reset()?;
            }
        }
        Ok(())
    }

    pub fn graceful_shutdown(&mut self) -> anyhow::Result<()> {
        self.reset()?;
        Ok(())
    }
}

impl Drop for Optimizer {
    fn drop(&mut self) {
        if let Err(why) = self.graceful_shutdown() {
            tracing::error!("Shutdown failed: {}", why);
        }
    }
}

// Try to reset everything, if fails, go on, set process state to a regular one as possible
fn reset_process(
    pid: nix::unistd::Pid,
    state: ProcessState,
    settings: &cfg::Settings,
) -> anyhow::Result<()> {
    tracing::info!("Resetting process: {}", pid.as_raw());

    if settings.niceness.enabled {
        if let Err(why) = scheduler::set_process_niceness(
            pid,
            state.niceness.unwrap_or(settings.niceness.default_value),
        ) {
            tracing::error!("Failed to reset process niceness: {}", why);
        }
    }

    if settings.ioniceness.enabled {
        if let Err(why) = io::set_process_io_niceness(
            pid,
            state
                .ioniceness
                .unwrap_or(settings.ioniceness.default_value),
        ) {
            tracing::error!("Failed to reset process I/O niceness: {}", why);
        }
    }

    if settings.cpu_affinity.enabled {
        match utils::get_process_tasks(pid) {
            Ok(tasks) => {
                let tasks = &tasks[1..]; // 0 task is the main thread, which we already pin
                for task in tasks {
                    let aff_mask = state.aff_mask.unwrap_or_else(|| get_aff_default().unwrap());
                    if let Err(why) =
                        cpu::set_aff_mask(nix::unistd::Pid::from_raw(*task as i32), aff_mask)
                    {
                        tracing::error!("Could not reset process affinity mask: {}", why);
                    }
                }
            }
            Err(why) => {
                tracing::error!("Fetching process's tasks failure: {}", why);
            }
        }
    }
    Ok(())
}

// Try to optimize everything, if it fails go on, in the end the old state will be a default state for a process and its tasks
fn optimize_process(pid: nix::unistd::Pid, settings: &cfg::Settings) {
    tracing::info!("Optimizing process: {}", pid.as_raw());

    if settings.niceness.enabled {
        if let Err(why) = scheduler::set_process_niceness(pid, settings.niceness.optimized_value) {
            tracing::error!("update process niceness failure: {}", why);
        }
    }
    if settings.ioniceness.enabled {
        if let Err(why) = io::set_process_io_niceness(pid, settings.ioniceness.optimized_value) {
            tracing::error!("update process ioniceness failure: {}", why);
        }
    }

    if settings.cpu_affinity.enabled {
        match cpu::cpus_load() {
            Ok(mut cpus_load) => {
                cpus_load.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                let mut cpu_idx = None;
                for (idx, _) in cpus_load.iter() {
                    match cpu::cpu_core_id(*idx) {
                        Ok(idx) => cpu_idx = Some(idx),
                        Err(why) => {
                            tracing::warn!("Checking logical core's hardware core failure: {}", why)
                        }
                    }
                }
                if cpu_idx == None {
                    tracing::error!("CPU Pinning failed for unknown reason");
                }

                if let Err(why) = cpu::pin_process(pid, cpu_idx.unwrap()) {
                    tracing::error!("Failed to pin {}: {}", pid.as_raw(), why);
                }
                match utils::get_process_tasks(pid) {
                    Ok(tasks) => {
                        let tasks = &tasks[1..]; // 0 task is the main thread, which we already pin
                        for task in tasks {
                            if let Err(why) = cpu::pin_process_excluding(
                                nix::unistd::Pid::from_raw(*task as i32),
                                cpu_idx.unwrap(),
                            ) {
                                tracing::error!("Pinning task {} failure: {}", task, why)
                            }
                        }
                    }
                    Err(why) => {
                        tracing::error!("Fetching process's tasks failure: {}", why);
                    }
                }
            }
            Err(why) => {
                tracing::error!("CPUs load measurement failed: {}", why);
            }
        }
    }
}

fn get_aff_default() -> anyhow::Result<libc::cpu_set_t> {
    let mut mask: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    let cpus_n = cpu::cpus_num()?;
    for i in 0..cpus_n as usize {
        unsafe {
            libc::CPU_SET(i, &mut mask);
        }
    }
    Ok(mask)
}
