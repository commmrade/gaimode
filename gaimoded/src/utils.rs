use nix::{sys::wait::waitpid, unistd};
use std::{
    os::fd::{AsFd, AsRawFd},
    process::exit,
};

pub const UDS_FILENAME: &'static str = "gaimoded_sock";

pub enum Commands {
    OptimizeProcess(nix::unistd::Pid),
    ResetProcess(nix::unistd::Pid),
    ResetAll,
}

#[allow(dead_code)]
pub fn daemonize() -> anyhow::Result<()> {
    match unsafe { unistd::fork() } {
        #[allow(unused)]
        Ok(unistd::ForkResult::Parent { child }) => {
            unsafe { nix::libc::_exit(0) };
        }
        Ok(unistd::ForkResult::Child) => {
            if let Err(why) = unistd::setsid() {
                return Err(anyhow::anyhow!("setsid failed: {}", why));
            }

            match unsafe { unistd::fork() } {
                Ok(unistd::ForkResult::Child) => {
                    // daemonized
                    let null_file = std::fs::OpenOptions::new()
                        .write(true)
                        .read(true)
                        .open("/dev/null")?;

                    nix::unistd::dup2_stdout(null_file.as_fd())?;
                    nix::unistd::dup2_stdin(null_file.as_fd())?;
                    nix::unistd::dup2_stderr(null_file.as_fd())?;

                    nix::unistd::chdir("/")?; // chdir to / as daemon

                    return Ok(());
                }
                Ok(unistd::ForkResult::Parent { .. }) => {
                    unsafe { nix::libc::_exit(0) };
                }
                Err(why) => return Err(anyhow::anyhow!("Fork failed: {}", why)),
            }
        }
        Err(why) => {
            tracing::error!("Fork failed: {}", why);
        }
    }

    Ok(())
}

pub fn tasks_in_process_n(pid: nix::unistd::Pid) -> anyhow::Result<u32> {
    let path = format!("/proc/{}/task", pid.as_raw());
    let dir = std::fs::read_dir(path)?;
    Ok(dir.count() as u32)
}

pub fn get_process_tasks(pid: nix::unistd::Pid) -> anyhow::Result<Vec<u32>> {
    let mut res = Vec::new();
    let path = format!("/proc/{}/task", pid.as_raw());
    let dir_iter = std::fs::read_dir(path)?;
    for dir in dir_iter {
        let task_id = dir?.file_name().to_string_lossy().parse::<u32>()?;
        res.push(task_id);
    }
    Ok(res)
}
