use std::{collections::HashMap, path::PathBuf, time::Duration};

use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    cpu, io, scheduler,
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
    aff_mask: libc::cpu_set_t, // Store main thread affinity mask, TODO: Implement
}
impl Default for ProcessState {
    fn default() -> Self {
        Self {
            niceness: None,
            ioniceness: None,
            aff_mask: unsafe { std::mem::zeroed() },
        }
    }
}

#[allow(dead_code)]
// TODO: Probably should make it a singleton or something, so that I can access it in signal handler | panic handler
pub struct Optimizer {
    old_sys_state: Option<Vec<State>>, // p
    processes: HashMap<nix::unistd::Pid, ProcessState>,
    is_optimized: bool,
}

impl Optimizer {
    pub fn new() -> Self {
        Self {
            old_sys_state: None,
            processes: HashMap::new(),
            is_optimized: false,
        }
    }

    fn optimize_cpu(&mut self) -> anyhow::Result<()> {
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
        Ok(())
    }
    fn reset_cpu(&mut self) -> anyhow::Result<()> {
        if let Some(old_state) = self.old_sys_state.as_ref() {
            for state in old_state {
                cpu::set_gov(&state.path, &state.governor)?;
            }
        }

        self.is_optimized = false;
        Ok(())
    }

    fn add_process(&mut self, pid: nix::unistd::Pid) -> anyhow::Result<()> {
        let old_niceness: i32 =
            scheduler::process_niceness(pid).unwrap_or(scheduler::DEFAULT_NICE_VALUE);
        let old_ioniceness: i32 = io::process_io_niceness(pid).unwrap_or(io::DEFAULT_IO_NICE_VALUE);

        let mut pstate = ProcessState::default();
        pstate.niceness = Some(old_niceness);
        pstate.ioniceness = Some(old_ioniceness);
        // TODO: store current affinity mask

        self.processes.insert(pid, pstate);

        optimize_process(pid)?;
        Ok(())
    }

    fn reset_processes(&mut self) -> anyhow::Result<()> {
        for (process, state) in self.processes.drain() {
            reset_process(process, state)?;
        }
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        println!("Reset optimizations");
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
            if res < 0 && res != nix::libc::EPERM {
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
        if let Ok(command) = rx.try_recv() {
            match command {
                utils::Commands::OptimizeProcess(pid) => {
                    if !self.is_optimized {
                        if let Err(_) = self.optimize_cpu() {
                            eprintln!("Your CPUFreq Policies do not support 'Performance' governor")
                        }
                        self.is_optimized = true;
                    }

                    self.add_process(pid)?;
                }
                utils::Commands::ResetProcess(pid) => {
                    if let Some(state) = self.processes.remove(&pid) {
                        reset_process(pid, state)?;
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
}

fn reset_process(pid: nix::unistd::Pid, state: ProcessState) -> anyhow::Result<()> {
    println!("Resetting process {}", pid.as_raw());
    if let Err(why) = scheduler::set_process_niceness(
        pid,
        state.niceness.unwrap_or(scheduler::DEFAULT_NICE_VALUE),
    ) {
        eprintln!("reset niceness failed: {}", why);
    }
    if let Err(why) =
        io::set_process_io_niceness(pid, state.ioniceness.unwrap_or(io::DEFAULT_IO_NICE_VALUE))
    {
        eprintln!("reset io niceness failed: {}", why);
    }

    // TODO: Reset CPU Pinning (AFfinity, parking) (and aff mask)
    Ok(())
}

fn optimize_process(pid: nix::unistd::Pid) -> anyhow::Result<()> {
    println!("Optimizing process {}", pid.as_raw());

    // nicenessness
    // We can kinda ignore an error, maybe if it fails this it doesnt fail to do any other optimizations
    if let Err(why) = scheduler::set_process_niceness(pid, scheduler::OPTIMIZED_NICE_VALUE) {
        eprintln!("Niceness failed: {}", why);
    }
    if let Err(why) = io::set_process_io_niceness(pid, io::OPTIMIZED_IO_NICE_VALUE) {
        eprintln!("IONiceness failed: {}", why);
    }

    // CPU Affinity
    // Find the lowest loaded cpu
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

    Ok(())
}
