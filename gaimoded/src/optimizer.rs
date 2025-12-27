use std::{collections::HashMap, path::PathBuf};

use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    cfg, cpu, io, scheduler,
    utils::{self},
};

#[allow(dead_code)]
struct State {
    path: PathBuf,
    governor: String,
}
impl Default for State {
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

#[allow(dead_code)]
// TODO: Probably should make it a singleton or something, so that I can access it in signal handler | panic handler
pub struct Optimizer {
    old_sys_state: Option<Vec<State>>, // p
    processes: HashMap<nix::unistd::Pid, ProcessState>,
    is_optimized: bool,
    settings: cfg::Settings,
}

impl Optimizer {
    pub fn new(settings: cfg::Settings) -> Self {
        Self {
            old_sys_state: None,
            processes: HashMap::new(),
            is_optimized: false,
            settings,
        }
    }

    fn optimize_cpu(&mut self) -> anyhow::Result<()> {
        if self.settings.cpu_governor.enabled {
            if !cpu::is_gov_available(cpu::PERF_GOV)? {
                return Err(anyhow::anyhow!(
                    "Your policies do not support 'Performance' governor"
                ));
            }

            let govs = cpu::get_govs()?;

            let mut new_old_global_state = Vec::new();
            new_old_global_state.reserve(govs.len());
            for (path, gov) in govs.into_iter() {
                let mut state = State::default();
                state.governor = gov;
                state.path = path;
                new_old_global_state.push(state);
            }
            self.old_sys_state = Some(new_old_global_state);

            cpu::set_gov_all(cpu::PERF_GOV)?;
        }
        Ok(())
    }
    fn reset_cpu(&mut self) -> anyhow::Result<()> {
        if self.settings.cpu_governor.enabled {
            if let Some(old_state) = self.old_sys_state.as_ref() {
                for state in old_state {
                    cpu::set_gov(&state.path, &state.governor)?;
                }
            }
        }

        self.is_optimized = false;
        Ok(())
    }

    fn add_process(&mut self, pid: nix::unistd::Pid) -> anyhow::Result<()> {
        let mut pstate = ProcessState::default();

        if self.settings.niceness.enabled {
            let old_niceness: i32 =
                scheduler::process_niceness(pid).unwrap_or(scheduler::DEFAULT_NICE_VALUE);
            pstate.niceness = Some(old_niceness);
        }
        if self.settings.ioniceness.enabled {
            let old_ioniceness: i32 =
                io::process_io_niceness(pid).unwrap_or(io::DEFAULT_IO_NICE_VALUE);
            pstate.ioniceness = Some(old_ioniceness);
        }
        if self.settings.cpu_affinity.enabled {
            pstate.aff_mask =
                Some(cpu::get_aff_mask(pid).unwrap_or_else(|_| get_aff_default().unwrap()));
        }

        self.processes.insert(pid, pstate);
        optimize_process(pid, &self.settings)?;
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
        if self.is_optimized {
            self.reset_cpu()?;
            self.reset_processes()?;
        }
        Ok(())
    }

    fn clear_dead_pids(&mut self) -> bool {
        let mut has_removed = false;
        self.processes.retain(|pid, _| {
            let res = unsafe { nix::libc::kill(pid.as_raw(), 0) };
            if res < 0 && unsafe { *libc::__errno_location() } != nix::libc::EPERM {
                // if it returns < 0 -> process does not exist, EPERM means process exist, but not enough perms to kill
                has_removed = true;
                return false;
            }
            return true;
        });
        has_removed
    }

    pub async fn process(
        &mut self,
        rx: &mut UnboundedReceiver<utils::Commands>,
    ) -> anyhow::Result<()> {
        while let Ok(command) = rx.try_recv() {
            match command {
                utils::Commands::OptimizeProcess(pid) => {
                    if !self.is_optimized {
                        if let Err(why) = self.optimize_cpu() {
                            tracing::error!("Error optimizing CPU: {}", why);
                        }
                        self.is_optimized = true;
                    }

                    self.add_process(pid)?;
                }
                utils::Commands::ResetProcess(pid) => {
                    if let Some(state) = self.processes.remove(&pid) {
                        reset_process(pid, state, &self.settings)?;
                    }
                }
                utils::Commands::ResetAll => self.reset()?,
            }
        }

        let processes_died = self.clear_dead_pids();
        if processes_died || (self.is_optimized && self.processes.is_empty()) {
            self.reset()?;
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
        self.graceful_shutdown().unwrap();
    }
}

fn reset_process(
    pid: nix::unistd::Pid,
    state: ProcessState,
    settings: &cfg::Settings,
) -> anyhow::Result<()> {
    tracing::info!("Resetting process: {}", pid.as_raw());

    if settings.niceness.enabled {
        if let Err(why) = scheduler::set_process_niceness(
            pid,
            state.niceness.unwrap_or(scheduler::DEFAULT_NICE_VALUE),
        ) {
            tracing::error!("Failed to reset process niceness: {}", why);
        }
    }

    if settings.ioniceness.enabled {
        if let Err(why) =
            io::set_process_io_niceness(pid, state.ioniceness.unwrap_or(io::DEFAULT_IO_NICE_VALUE))
        {
            tracing::error!("Failed to reset process I/O niceness: {}", why);
        }
    }

    if settings.cpu_affinity.enabled {
        let tasks = &utils::get_process_tasks(pid)?; // 0 task is the process itself (main thread)
        for task in tasks {
            if let Err(why) = cpu::set_aff_mask(
                nix::unistd::Pid::from_raw(*task as i32),
                state.aff_mask.unwrap_or_else(|| get_aff_default().unwrap()),
            ) {
                tracing::error!("Could not reset process affinity mask: {}", why);
            }
        }
    }
    Ok(())
}

fn optimize_process(pid: nix::unistd::Pid, settings: &cfg::Settings) -> anyhow::Result<()> {
    tracing::info!("Optimizing process: {}", pid.as_raw());

    // nicenessness
    // We can kinda ignore an error, maybe if it fails this it doesnt fail to do any other optimizations
    if settings.niceness.enabled {
        if let Err(why) = scheduler::set_process_niceness(pid, scheduler::OPTIMIZED_NICE_VALUE) {
            tracing::error!("Failed to set niceness, not nice: {}", why);
        }
    }
    if settings.ioniceness.enabled {
        if let Err(why) = io::set_process_io_niceness(pid, io::OPTIMIZED_IO_NICE_VALUE) {
            tracing::error!("Failed to set I/O niceness, not ionice: {}", why);
        }
    }

    // CPU Affinity
    // Find the lowest loaded cpu
    if settings.cpu_affinity.enabled {
        if let Ok(mut cpu_loads) = cpu::cpus_load() {
            cpu_loads.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

            let mut cpu_idx = 0;
            for (idx, _) in cpu_loads.iter() {
                // Note: shouldn't pin to core 0 since it is heavily used by the kernel for OS stuff
                if cpu::cpu_core_id(*idx)? > 0 {
                    cpu_idx = *idx;
                    break;
                }
            }

            cpu::pin_process(pid, cpu_idx)?;
            let tasks = &utils::get_process_tasks(pid)?[1..]; // 0 task is the process itself (main thread)
            for task in tasks {
                cpu::pin_process_excluding(nix::unistd::Pid::from_raw(*task as i32), cpu_idx)?;
            }
        }
    }

    Ok(())
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
