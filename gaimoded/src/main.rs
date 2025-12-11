use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
mod cpu;
mod signals;
mod utils;

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
    // #[cfg(target_os = "linux")]
    // ioniceness: i32,
    // TODO: more fields? and linux specific fields
}
impl Default for ProcessState {
    fn default() -> Self {
        Self {
            niceness: 0,
            // #[cfg(target_os = "linux")]
            // ioniceness: 0,
        }
    }
}

#[allow(dead_code)]
// TODO: Probably should make it a singleton or something, so that I can access it in signal handler | panic handler
struct Optimizer {
    old_pol_state: Option<Vec<State>>, // p
    processes: HashMap<nix::unistd::Pid, ProcessState>,
    is_optimized: bool,
}

impl Optimizer {
    pub fn new() -> Self {
        Self {
            old_pol_state: None,
            processes: HashMap::new(),
            is_optimized: false,
        }
    }

    fn optimize_cpu(&mut self) -> anyhow::Result<()> {
        let govs = cpu::get_govs()?;

        let mut new_old_global_state = Vec::new();
        new_old_global_state.reserve(govs.len());
        for (path, gov) in govs.into_iter() {
            let mut state = State::default();
            state.governor = gov;
            state.path = path;
            new_old_global_state.push(state);
        }
        self.old_pol_state = Some(new_old_global_state);

        if !cpu::is_gov_available(cpu::PERF_GOV)? {
            return Err(anyhow::anyhow!(
                "Your policies do not support 'Performance' governor"
            ));
        }
        cpu::set_gov_all(cpu::PERF_GOV)?;
        Ok(())
    }
    fn reset_cpu(&mut self) -> anyhow::Result<()> {
        if let Some(old_state) = self.old_pol_state.as_ref() {
            for state in old_state {
                cpu::set_gov(&state.path, &state.governor)?; // TODO: handle
            }
        }

        // TODO: Iterate processes and reset niceness
        self.is_optimized = false;
        Ok(())
    }

    fn optimize_process(&mut self, pid: nix::unistd::Pid) {
        println!("Optimizing process {}", pid.as_raw());
        // TODO: get process state
        self.processes.insert(pid, ProcessState::default());

        // TODO: optimize
    }
    fn reset_process(&mut self, pid: nix::unistd::Pid) {
        println!("Resetting process {}", pid.as_raw());
        self.processes.remove(&pid);

        // Reset process settings
    }
    fn reset_processes(&mut self) -> anyhow::Result<()> {
        // todo: reset
        for (process, state) in self.processes.drain() {
            // Clear
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

    pub async fn worker(mut self, mut rx: UnboundedReceiver<utils::Commands>) {
        loop {
            if let Ok(command) = rx.try_recv() {
                match command {
                    utils::Commands::OptimizeProcess(pid) => {
                        if !self.is_optimized {
                            if let Err(_) = self.optimize_cpu() {
                                eprintln!(
                                    "Your CPUFreq Policies do not support 'Performance' governor"
                                )
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
}

async fn uds_worker(listener: tokio::net::UnixListener, tx: UnboundedSender<utils::Commands>) {
    loop {
        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                let mut buf: [u8; 2048] = [0u8; 2048];
                stream.read(&mut buf).await.unwrap();

                let packet = gaiproto::Gaiproto::from_bytes(buf.to_vec());
                if let Err(why) = handle_packet(&packet, tx.clone()).await {
                    eprintln!("Handle packet failed: {}", why);
                }
            }
            Err(why) => {
                eprintln!("Accept failed: {}", why);
            }
        }
    }
}

async fn handle_packet(
    pkt: &gaiproto::Gaiproto,
    tx: UnboundedSender<utils::Commands>,
) -> anyhow::Result<()> {
    match pkt.kind {
        gaiproto::K_OPTIMIZE_PROCESS => {
            let pid_raw = i32::from_be_bytes(pkt.payload.clone().try_into().unwrap());
            let pid = nix::unistd::Pid::from_raw(pid_raw);
            tx.send(utils::Commands::OptimizeProcess(pid)).unwrap();
        }
        gaiproto::K_RESET_PROCESS => {
            let pid_raw = i32::from_be_bytes(pkt.payload.clone().try_into().unwrap());
            let pid = nix::unistd::Pid::from_raw(pid_raw);
            tx.send(utils::Commands::ResetProcess(pid)).unwrap();
        }
        gaiproto::K_RESET_ALL => {
            tx.send(utils::Commands::ResetAll).unwrap();
        }
        _ => {
            // Ignore
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    // if let Err(why) = daemonize() {
    //     eprintln!("Daemonize failed: {}", why);
    //     return;
    // }

    let mut path = std::env::temp_dir();
    path.push(utils::UDS_FILENAME);
    if path.exists() {
        std::fs::remove_file(&path).unwrap();
    }

    let listener = tokio::net::UnixListener::bind(&path).expect("UDS creation failed");
    let perms = std::fs::Permissions::from_mode(0x666);
    std::fs::set_permissions(&path, perms).expect("chmod failed");

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<utils::Commands>();

    tokio::spawn(async move {
        let optimizer = Optimizer::new();
        optimizer.worker(rx).await;
    });
    uds_worker(listener, tx).await;

    nix::unistd::unlink(&path).unwrap(); // TODO: move somewhere else, it should be done on exit
}
