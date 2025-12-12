use std::process::exit;

use nix::{sys::wait::waitpid, unistd};

use crate::cpu;

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

pub const OPTIMIZED_NICE_VALUE: i32 = -10;

// TOOD: Move to scheduling i think since it changes scheduling of a process
pub fn process_niceness(pid: nix::unistd::Pid) -> anyhow::Result<i32> {
    unsafe {
        /*
         *   Since a successful call to getpriority() can legitimately return
         *   the value -1, it is necessary to clear errno prior to the call,
         *   then check errno afterward to determine if -1 is an error or a
         *   legitimate value.
         */
        *libc::__errno_location() = 0;
        let ret = libc::getpriority(libc::PRIO_PROCESS, pid.as_raw() as u32);

        // If returned -1 and errno is. Not sure if it's thread safe, but no other thread seem to be messing with errno
        if ret == -1 && *libc::__errno_location() > 0 {
            eprintln!("Could not get process niceness");
        }
        Ok(ret)
    }
}

pub fn set_process_niceness(pid: nix::unistd::Pid, niceness: i32) -> anyhow::Result<()> {
    unsafe {
        let ret = libc::setpriority(libc::PRIO_PROCESS, pid.as_raw() as u32, niceness);
        if ret < 0 {
            return Err(anyhow::anyhow!("Failed to change process niceness"));
        }
        Ok(())
    }
}
