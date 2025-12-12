use nix::{sys::wait::waitpid, unistd};
use std::process::exit;

pub const UDS_FILENAME: &'static str = "gaimoded_sock";

pub enum Commands {
    OptimizeProcess(nix::unistd::Pid),
    ResetProcess(nix::unistd::Pid),
    ResetAll,
}

#[allow(dead_code)]
pub fn daemonize() -> anyhow::Result<()> {
    match unsafe { unistd::fork() } {
        Ok(unistd::ForkResult::Parent { child }) => {
            if let Err(why) = waitpid(child, None) {
                return Err(anyhow::anyhow!("Failed to waitpid: {}", why));
            }
            exit(0);
        }
        Ok(unistd::ForkResult::Child) => {
            if let Err(why) = unistd::setsid() {
                return Err(anyhow::anyhow!("setsid failed: {}", why));
            }

            match unsafe { unistd::fork() } {
                Ok(unistd::ForkResult::Child) => {
                    // daemonized
                    return Ok(());
                }
                Ok(unistd::ForkResult::Parent { .. }) => {
                    exit(0);
                }
                Err(why) => return Err(anyhow::anyhow!("Fork failed: {}", why)),
            }
        }
        Err(why) => {
            eprintln!("Fork failed: {}", why);
        }
    }

    Ok(())
}

pub fn tasks_in_process(pid: nix::unistd::Pid) -> anyhow::Result<u32> {
    let path = format!("/proc/{}/task", pid.as_raw());
    let dir = std::fs::read_dir(path)?;
    Ok(dir.count() as u32)
}
