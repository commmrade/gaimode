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
    // TODO: more fields?
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
    niceness: i32,
    ioniceness: i32,
    // TODO: more fields?
}
impl Default for ProcessState {
    fn default() -> Self {
        Self {
            niceness: 0,
            ioniceness: 0,
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
                cpu::set_gov(&state.path, &state.governor)?; // TODO: handle
            }
        }

        self.is_optimized = false;
        Ok(())
    }

    fn optimize_process(&mut self, pid: nix::unistd::Pid) {
        println!("Optimizing process {}", pid.as_raw());
        // TODO: get process state
        let old_niceness: i32 = scheduler::process_niceness(pid).unwrap();
        let old_ioniceness: i32 = io::process_io_niceness(pid).unwrap();

        let mut pstate = ProcessState::default();
        pstate.niceness = old_niceness;
        pstate.ioniceness = old_ioniceness;

        self.processes.insert(pid, pstate);

        scheduler::set_process_niceness(pid, scheduler::OPTIMIZED_NICE_VALUE).unwrap();
        io::set_process_io_niceness(pid, io::OPTIMIZED_IO_NICE_VALUE).unwrap();
    }
    fn reset_process(&mut self, pid: nix::unistd::Pid) {
        println!("Resetting process {}", pid.as_raw());
        let state = self.processes.remove(&pid).unwrap(); // Safety: Process should exist

        scheduler::set_process_niceness(pid, state.niceness).unwrap(); // TODO: Handle
        io::set_process_io_niceness(pid, state.ioniceness).unwrap();
        // ...
    }
    fn reset_processes(&mut self) -> anyhow::Result<()> {
        // todo: reset
        for (process, state) in self.processes.drain() {
            // Clear niceness, ioniceness and yada yada
            // TODO: Factor out, a lot of duplication of code
            scheduler::set_process_niceness(process, state.niceness).unwrap();
            io::set_process_io_niceness(process, state.ioniceness).unwrap();
            // ...
        }
        Ok(())
    }

    fn reset(&mut self) {
        println!("Reset optimizations");
        if self.is_optimized {
            self.reset_cpu().unwrap();
            self.reset_processes().unwrap();
        }
        // todo!("Reset all kinds of optimizations");
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

    pub async fn process(&mut self, rx: &mut UnboundedReceiver<utils::Commands>) {
        if let Ok(command) = rx.try_recv() {
            match command {
                utils::Commands::OptimizeProcess(pid) => {
                    if !self.is_optimized {
                        if let Err(_) = self.optimize_cpu() {
                            eprintln!("Your CPUFreq Policies do not support 'Performance' governor")
                        }
                        self.is_optimized = true;
                    }

                    self.optimize_process(pid);
                }
                utils::Commands::ResetProcess(pid) => self.reset_process(pid),
                utils::Commands::ResetAll => self.reset(),
            }
        }

        let processes_died = self.clear_dead_pids();
        if processes_died && self.processes.is_empty() {
            self.reset();
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
