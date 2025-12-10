use std::{collections::HashMap, time::Duration};
use tokio::{
    io::AsyncReadExt,
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};
mod signals;
mod utils;

#[allow(dead_code)]
struct State {
    cpu_gov: String,
    // TODO: more fields?
}
impl Default for State {
    fn default() -> Self {
        Self {
            cpu_gov: String::new(),
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
struct Optimizer {
    old_global_state: Option<State>,
    processes: HashMap<nix::unistd::Pid, ProcessState>,
    is_optimized: bool,
}

impl Optimizer {
    pub fn new() -> Self {
        Self {
            old_global_state: None,
            processes: HashMap::new(),
            is_optimized: false,
        }
    }

    fn optimize_process(&mut self, pid: nix::unistd::Pid) {
        println!("Optimizing process {}", pid.as_raw());
        self.processes.insert(pid, ProcessState::default());
        self.is_optimized = true;
    }
    fn reset_process(&mut self, pid: nix::unistd::Pid) {
        println!("Resetting process {}", pid.as_raw());
        self.processes.remove(&pid);
    }

    fn reset(&mut self) {
        println!("Reset everything");
        // todo!("Reset all kinds of optimizations");
    }

    fn check_pids(&mut self) -> bool {
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
                    utils::Commands::OptimizeProcess(pid) => self.optimize_process(pid),
                    utils::Commands::ResetProcess(pid) => self.reset_process(pid),
                    utils::Commands::ResetAll => self.reset(),
                }
            }

            let processes_died = self.check_pids();
            if processes_died && self.processes.is_empty() {
                self.reset();
            }

            tokio::time::sleep(Duration::from_secs(3)).await;
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

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<utils::Commands>();

    tokio::spawn(async move {
        let optimizer = Optimizer::new();
        optimizer.worker(rx).await;
    });
    uds_worker(listener, tx).await;

    nix::unistd::unlink(&path).unwrap(); // TODO: move somewhere else, it should be done on exit
}
