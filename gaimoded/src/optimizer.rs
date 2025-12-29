use std::{collections::HashMap, path::PathBuf};

use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    cfg, cpu, io, scheduler,
    utils::{self},
};

/*
* https://grok.com/share/bGVnYWN5_1ed88943-3d18-49fb-8123-76e63e7124b7 - list of improvements i can do
*/

#[allow(dead_code)]
struct ProcessState {
    niceness: Option<i32>,
    ioniceness: Option<i32>,
    aff_mask: Option<libc::cpu_set_t>, // Store main thread affinity mask
}

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
struct SystemState {
    cpus_state: Vec<CpuState>,
}

pub trait OptimizerObject {
    fn optimize(&mut self) -> anyhow::Result<()>;
    fn unoptimize(&mut self) -> anyhow::Result<()>;
}

struct SystemOptimizer {
    state: SystemState,
    settings: cfg::Settings,
}

impl OptimizerObject for SystemOptimizer {
    fn optimize(&mut self) -> anyhow::Result<()> {
        if self.settings.cpu_governor.enabled {
            if !cpu::is_gov_available(cpu::PERF_GOV)? {
                return Err(anyhow::anyhow!("Performance governor is not supported"));
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
        }
        Ok(())
    }
    fn unoptimize(&mut self) -> anyhow::Result<()> {
        let mut failed = false;
        if self.settings.cpu_governor.enabled {
            for state in &self.state.cpus_state {
                if let Err(why) = cpu::set_gov(&state.path, &state.governor) {
                    tracing::error!("Failed to set gov: {}", why);
                    failed = true;
                }
            }
        }
        if failed {
            return Err(anyhow::anyhow!("One or more cpu govs could not be reset"));
        }
        Ok(())
    }
}

impl SystemOptimizer {
    pub fn new(settings: cfg::Settings) -> Self {
        Self {
            state: SystemState {
                cpus_state: Vec::new(),
            },
            settings,
        }
    }
}

struct ProcessOptimizer {
    pid: nix::unistd::Pid,
    old_state: ProcessState,
    settings: cfg::Settings,
}

impl OptimizerObject for ProcessOptimizer {
    fn optimize(&mut self) -> anyhow::Result<()> {
        if self.settings.niceness.enabled {
            if let Err(why) =
                scheduler::set_process_niceness(self.pid, self.settings.niceness.optimized_value)
            {
                tracing::error!("nicencess change failed: {}", why);
                return Err(why.into());
            }
        }
        if self.settings.ioniceness.enabled {
            if let Err(why) =
                io::set_process_io_niceness(self.pid, self.settings.ioniceness.optimized_value)
            {
                tracing::error!("ioniceness change failed: {}", why);

                if let Err(why) = self.rollback_nc() {
                    tracing::error!("rollback failed: {}", why);
                }
                return Err(why.into());
            }
        }

        if self.settings.cpu_affinity.enabled {
            match cpu::cpus_load() {
                Ok(mut cpus_load) => {
                    cpus_load.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

                    let mut cpu_idx = None;
                    for (idx, _) in cpus_load.iter() {
                        if let Ok(core_id) = cpu::cpu_core_id(*idx) {
                            if core_id > 0 {
                                cpu_idx = Some(idx);
                            }
                        }
                    }
                    if cpu_idx.is_none() {
                        if let Err(why) = self.rollback_nc_ionc() {
                            tracing::error!("Rollback failed: {}", why);
                        }
                        return Err(anyhow::anyhow!("a logic core for process was not chosen"));
                    }

                    if let Err(why) = cpu::pin_process(self.pid, *cpu_idx.unwrap()) {
                        if let Err(why) = self.rollback_nc_ionc() {
                            tracing::error!("Rollback failed: {}", why);
                        }
                        return Err(why.into());
                    }

                    match utils::get_process_tasks(self.pid) {
                        Ok(tasks) => {
                            let tasks = &tasks[1..]; // 0 task is the main thread, which we already pin
                            for task in tasks {
                                if let Err(why) = cpu::pin_process_excluding(
                                    nix::unistd::Pid::from_raw(*task as i32),
                                    *cpu_idx.unwrap(),
                                ) {
                                    if let Err(why) = self.rollback_nc_ionc() {
                                        tracing::error!("Rollback failed: {}", why);
                                    }
                                    return Err(why.into());
                                }
                            }
                        }
                        Err(why) => {
                            if let Err(why) = self.rollback_nc_ionc() {
                                tracing::error!("Rollback failed: {}", why);
                            }
                            return Err(why.into());
                        }
                    }
                }
                Err(why) => {
                    tracing::error!("Failed to get CPUs load: {}", why);

                    if let Err(why) = self.rollback_nc_ionc() {
                        tracing::error!("Rollback failed: {}", why);
                    }
                    return Err(why.into());
                }
            }
        }

        Ok(())
    }

    fn unoptimize(&mut self) -> anyhow::Result<()> {
        // Когда оптимизирую, при фейле делаю роллбэк, а что делать при аноптимайз если фейлится? забить можно
        self.rollback_nc_ionc()?;

        if self.settings.cpu_affinity.enabled {
            // if it fails idc
            let tasks = utils::get_process_tasks(self.pid)?;

            for task in tasks {
                let aff_mask = self
                    .old_state
                    .aff_mask
                    .unwrap_or_else(|| get_aff_default().unwrap());
                cpu::set_aff_mask(nix::unistd::Pid::from_raw(task as i32), aff_mask)?;
            }
        }
        Ok(())
    }
}

impl ProcessOptimizer {
    pub fn from_process(pid: nix::unistd::Pid, settings: cfg::Settings) -> anyhow::Result<Self> {
        let mut pstate = ProcessState {
            niceness: None,
            ioniceness: None,
            aff_mask: None,
        };
        if settings.niceness.enabled {
            let niceness = scheduler::process_niceness(pid)?;
            pstate.niceness = Some(niceness);
        }
        if settings.ioniceness.enabled {
            let io_niceness = io::process_io_niceness(pid)?;
            pstate.ioniceness = Some(io_niceness);
        }

        if settings.cpu_affinity.enabled {
            let mask = cpu::get_aff_mask(pid)?;
            pstate.aff_mask = Some(mask);
        }
        Ok(Self {
            pid,
            old_state: pstate,
            settings,
        })
    }

    fn rollback_nc(&mut self) -> anyhow::Result<()> {
        if self.settings.niceness.enabled {
            scheduler::set_process_niceness(
                self.pid,
                self.old_state
                    .niceness
                    .unwrap_or(self.settings.niceness.default_value),
            )?;
        }
        Ok(())
    }
    fn rollback_nc_ionc(&mut self) -> anyhow::Result<()> {
        if self.settings.niceness.enabled {
            scheduler::set_process_niceness(
                self.pid,
                self.old_state
                    .niceness
                    .unwrap_or(self.settings.niceness.default_value),
            )?;
        }
        if self.settings.ioniceness.enabled {
            io::set_process_io_niceness(
                self.pid,
                self.old_state
                    .ioniceness
                    .unwrap_or(self.settings.ioniceness.default_value),
            )?;
        }
        Ok(())
    }
}

pub struct Optimizer {
    sys: SystemOptimizer,
    processes: HashMap<nix::unistd::Pid, ProcessOptimizer>,
    settings: cfg::Settings,
    is_active: bool,
}

impl OptimizerObject for Optimizer {
    fn optimize(&mut self) -> anyhow::Result<()> {
        self.sys.optimize()?;
        self.is_active = true;
        Ok(())
    }
    fn unoptimize(&mut self) -> anyhow::Result<()> {
        let mut failed = false;

        if let Err(why) = self.sys.unoptimize() {
            tracing::error!("Could not unoptimize system state: {}", why);
            failed = true;
        }

        for (_, mut process) in self.processes.drain() {
            if let Err(why) = process.unoptimize() {
                tracing::error!("Process unoptimization failed: {}", why);
                failed = true;
            }
        }
        if failed {
            return Err(anyhow::anyhow!("Failed to unoptimize fully"));
        }
        Ok(())
    }
}

impl Optimizer {
    pub fn new(settings: cfg::Settings) -> Self {
        Self {
            is_active: false,
            sys: SystemOptimizer::new(settings.clone()),
            processes: HashMap::new(),
            settings,
        }
    }

    pub async fn process(
        &mut self,
        rx: &mut UnboundedReceiver<utils::Commands>,
    ) -> anyhow::Result<()> {
        if let Ok(command) = rx.try_recv() {
            match command {
                utils::Commands::OptimizeProcess(pid) => {
                    if !self.is_active {
                        // If fails, it logs an error and prevents
                        self.optimize()?;
                    }

                    // Only add a new process if state is changed
                    if self.is_active {
                        let mut process =
                            ProcessOptimizer::from_process(pid, self.settings.clone())?;
                        process.optimize()?;
                        self.processes.insert(pid, process);
                    }
                }
                utils::Commands::ResetProcess(pid) => {}
                utils::Commands::ResetAll => {
                    self.unoptimize()?;
                }
            }

            todo!("CLear dead processes");
            todo!("Check if should reset optimizations (when no processes)")
        }

        Ok(())
    }
}

// struct CpuState {
//     path: PathBuf,
//     governor: String,
// }
// impl Default for CpuState {
//     fn default() -> Self {
//         Self {
//             path: PathBuf::new(),
//             governor: String::new(),
//         }
//     }
// }

// impl Default for ProcessState {
//     fn default() -> Self {
//         Self {
//             niceness: None,
//             ioniceness: None,
//             aff_mask: None,
//         }
//     }
// }

// struct SystemState {
//     cpus_state: Vec<CpuState>,
// }

// #[allow(dead_code)]
// pub struct Optimizer {
//     system_state: Option<SystemState>, // optimizer state basically
//     processes: HashMap<nix::unistd::Pid, ProcessState>,
//     settings: cfg::Settings,
// }

// impl Optimizer {
//     pub fn new(settings: cfg::Settings) -> Self {
//         Self {
//             system_state: None,
//             processes: HashMap::new(),
//             settings,
//         }
//     }

//     fn optimize(&mut self) -> anyhow::Result<()> {
//         if self.settings.cpu_governor.enabled {
//             let is_perf_supported = cpu::is_gov_available(cpu::PERF_GOV)?;
//             if !is_perf_supported {
//                 return Err(anyhow::anyhow!(
//                     "Your policies do not support 'Performance' governor"
//                 ));
//             }

//             let govs = cpu::get_govs()?;

//             let mut cpus_state = Vec::new();
//             cpus_state.reserve(govs.len());
//             for (path, gov) in govs.into_iter() {
//                 let mut state = CpuState::default();
//                 state.governor = gov;
//                 state.path = path;
//                 cpus_state.push(state);
//             }

//             cpu::set_gov_all(cpu::PERF_GOV)?;
//             self.system_state = Some(SystemState { cpus_state })
//         }
//         Ok(())
//     }

//     fn reset_system(&mut self) -> anyhow::Result<()> {
//         if self.settings.cpu_governor.enabled {
//             if let Some(state) = self.system_state.as_ref() {
//                 for cur in &state.cpus_state {
//                     // If 1 fails, try to reset all other policies
//                     if let Err(why) = cpu::set_gov(&cur.path, &cur.governor) {
//                         tracing::error!(
//                             "Setting gov for {} failed: {}",
//                             cur.path.to_string_lossy(),
//                             why
//                         );
//                     }
//                 }
//             }
//         }
//         Ok(())
//     }
//     fn reset_processes(&mut self) -> anyhow::Result<()> {
//         for (process, state) in self.processes.drain() {
//             reset_process(process, state, &self.settings)?;
//         }
//         Ok(())
//     }

//     fn reset(&mut self) -> anyhow::Result<()> {
//         tracing::info!("Resetting all optimizations");
//         if let Err(why) = self.reset_processes() {
//             tracing::error!("Reset processes failure: {}", why);
//         }
//         if let Err(why) = self.reset_system() {
//             tracing::error!("Reset system failure: {}", why);
//         }
//         self.system_state = None;
//         Ok(())
//     }

//     fn clear_dead_pids(&mut self) -> bool {
//         let mut has_removed = false;
//         self.processes.retain(|pid, _| {
//             // let res = unsafe { nix::libc::kill(pid.as_raw(), 0) }
//             match nix::sys::signal::kill(*pid, None) {
//                 Ok(_) => return true,                         // process alive
//                 Err(nix::errno::Errno::EPERM) => return true, // alive, but not enough permissions to send the signal
//                 Err(_) => {
//                     has_removed = true;
//                     return false;
//                 }
//             };
//         });
//         has_removed
//     }

//     fn add_process(&mut self, pid: nix::unistd::Pid) -> anyhow::Result<()> {
//         let mut pstate = ProcessState::default();

//         if self.settings.niceness.enabled {
//             match scheduler::process_niceness(pid) {
//                 Ok(nc) => pstate.niceness = Some(nc),
//                 Err(why) => {
//                     tracing::warn!("Getting process niceness failure: {}", why);
//                 }
//             }
//         }
//         if self.settings.ioniceness.enabled {
//             match io::process_io_niceness(pid) {
//                 Ok(nc) => pstate.ioniceness = Some(nc),
//                 Err(why) => {
//                     tracing::warn!("Getting process ioniceness failure: {}", why);
//                 }
//             }
//         }
//         if self.settings.cpu_affinity.enabled {
//             match cpu::get_aff_mask(pid) {
//                 Ok(mask) => pstate.aff_mask = Some(mask),
//                 Err(why) => {
//                     tracing::warn!("Getting affinity mask failure: {}", why);
//                 }
//             }
//         }

//         optimize_process(pid, &self.settings);
//         self.processes.insert(pid, pstate);
//         Ok(())
//     }

//     pub async fn process(
//         &mut self,
//         rx: &mut UnboundedReceiver<utils::Commands>,
//     ) -> anyhow::Result<()> {
//         if let Ok(command) = rx.try_recv() {
//             match command {
//                 utils::Commands::OptimizeProcess(pid) => {
//                     if self.system_state.is_none() {
//                         self.optimize()?;
//                     }
//                     self.add_process(pid)?;
//                 }
//                 utils::Commands::ResetProcess(pid) => {
//                     if let Some(state) = self.processes.remove(&pid) {
//                         reset_process(pid, state, &self.settings)?;
//                     }
//                 }
//                 utils::Commands::ResetAll => {
//                     self.reset()?;
//                 }
//             }
//         }

//         self.clear_dead_pids();
//         if let Some(_) = self.system_state.as_ref() {
//             if self.processes.is_empty() {
//                 // No processes to track, so reset the system state
//                 self.reset()?;
//             }
//         }
//         Ok(())
//     }

//     pub fn graceful_shutdown(&mut self) -> anyhow::Result<()> {
//         self.reset()?;
//         Ok(())
//     }
// }

// impl Drop for Optimizer {
//     fn drop(&mut self) {
//         if let Err(why) = self.graceful_shutdown() {
//             tracing::error!("Shutdown failed: {}", why);
//         }
//     }
// }

// // Try to reset everything, if fails, go on, set process state to a regular one as possible
// fn reset_process(
//     pid: nix::unistd::Pid,
//     state: ProcessState,
//     settings: &cfg::Settings,
// ) -> anyhow::Result<()> {
//     tracing::info!("Resetting process: {}", pid.as_raw());

//     if settings.niceness.enabled {
//         if let Err(why) = scheduler::set_process_niceness(
//             pid,
//             state.niceness.unwrap_or(settings.niceness.default_value),
//         ) {
//             tracing::error!("Failed to reset process niceness: {}", why);
//         }
//     }

//     if settings.ioniceness.enabled {
//         if let Err(why) = io::set_process_io_niceness(
//             pid,
//             state
//                 .ioniceness
//                 .unwrap_or(settings.ioniceness.default_value),
//         ) {
//             tracing::error!("Failed to reset process I/O niceness: {}", why);
//         }
//     }

//     if settings.cpu_affinity.enabled {
//         match utils::get_process_tasks(pid) {
//             Ok(tasks) => {
//                 for task in tasks {
//                     let aff_mask = state.aff_mask.unwrap_or_else(|| get_aff_default().unwrap());
//                     if let Err(why) =
//                         cpu::set_aff_mask(nix::unistd::Pid::from_raw(*task as i32), aff_mask)
//                     {
//                         tracing::error!("Could not reset process affinity mask: {}", why);
//                     }
//                 }
//             }
//             Err(why) => {
//                 tracing::error!("Fetching process's tasks failure: {}", why);
//             }
//         }
//     }
//     Ok(())
// }

// // Try to optimize everything, if it fails go on, in the end the old state will be a default state for a process and its tasks
// fn optimize_process(pid: nix::unistd::Pid, settings: &cfg::Settings) {
//     tracing::info!("Optimizing process: {}", pid.as_raw());

//     if settings.niceness.enabled {
//         if let Err(why) = scheduler::set_process_niceness(pid, settings.niceness.optimized_value) {
//             tracing::error!("update process niceness failure: {}", why);
//         }
//     }
//     if settings.ioniceness.enabled {
//         if let Err(why) = io::set_process_io_niceness(pid, settings.ioniceness.optimized_value) {
//             tracing::error!("update process ioniceness failure: {}", why);
//         }
//     }

//     if settings.cpu_affinity.enabled {
//         match cpu::cpus_load() {
//             Ok(mut cpus_load) => {
//                 cpus_load.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
//                 let mut cpu_idx = None;
//                 for (idx, _) in cpus_load.iter() {
//                     match cpu::cpu_core_id(*idx) {
//                         Ok(core_id) => {
//                             if *idx > 0 {
//                                 cpu_idx = Some(idx);
//                                 break;
//                             }
//                         }
//                         Err(why) => {
//                             tracing::warn!("Checking logical core's hardware core failure: {}", why)
//                         }
//                     }
//                 }
//                 if cpu_idx == None {
//                     tracing::error!("CPU Pinning failed for unknown reason");
//                 }

//                 if let Err(why) = cpu::pin_process(pid, *cpu_idx.unwrap()) {
//                     tracing::error!("Failed to pin {}: {}", pid.as_raw(), why);
//                 }
//                 match utils::get_process_tasks(pid) {
//                     Ok(tasks) => {
//                         let tasks = &tasks[1..]; // 0 task is the main thread, which we already pin
//                         for task in tasks {
//                             if let Err(why) = cpu::pin_process_excluding(
//                                 nix::unistd::Pid::from_raw(*task as i32),
//                                 *cpu_idx.unwrap(),
//                             ) {
//                                 tracing::error!("Pinning task {} failure: {}", task, why)
//                             }
//                         }
//                     }
//                     Err(why) => {
//                         tracing::error!("Fetching process's tasks failure: {}", why);
//                     }
//                 }
//             }
//             Err(why) => {
//                 tracing::error!("CPUs load measurement failed: {}", why);
//             }
//         }
//     }
// }

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
