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
pub const OPTIMIZED_IO_NICE_VALUE: i32 = 1;

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
        if ret == -1 && *libc::__errno_location() != 0 {
            return Err(anyhow::anyhow!("Could not get process niceness"));
        }
        Ok(ret)
    }
}

// Optimizes scheduling of a process
pub fn set_process_niceness(pid: nix::unistd::Pid, niceness: i32) -> anyhow::Result<()> {
    unsafe {
        // Task contains process itself
        let tasks_path = format!("/proc/{}/task/", pid.as_raw());
        let dir_iter = std::fs::read_dir(&tasks_path)?;
        for task in dir_iter {
            let task_tid = task?.file_name().to_string_lossy().parse::<u32>()?;

            let ret = libc::setpriority(libc::PRIO_PROCESS, task_tid, niceness);
            if ret < 0 {
                eprintln!("Failed to change TID niceness");
            }
        }
        Ok(())
    }
}

pub const IOPRIO_WHO_PROCESS: i32 = 1;
const IOPRIO_CLASS_SHIFT: i32 = 13;
pub const IOPRIO_PRIO_MASK: i32 = ((1 << IOPRIO_CLASS_SHIFT) - 1);
pub const IOPRIO_CLASS_BE: i32 = 2;

#[inline]
fn ioprio_prio_data(ioprio: i32) -> i32 {
    (ioprio) & IOPRIO_PRIO_MASK
}

#[inline]
fn ioprio_value(prioclass: i32, priolevel: i32) -> u16 {
    ((prioclass << IOPRIO_CLASS_SHIFT) | priolevel) as u16
}

pub fn process_io_niceness(pid: nix::unistd::Pid) -> anyhow::Result<i32> {
    unsafe {
        *libc::__errno_location() = 0;
        let ret = libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, pid.as_raw());
        if ret == -1 && *libc::__errno_location() != 0 {
            return Err(anyhow::anyhow!("Failed to get process IO niceness"));
        }

        // todo!("Extract data from IOPRIO value (ret)");
        Ok(ioprio_prio_data(ret as i32))
    }
}

// Optimizes IO performance when game has to load assets
pub fn set_process_io_niceness(pid: nix::unistd::Pid, ioniceness: i32) -> anyhow::Result<()> {
    unsafe {
        // todo!("IOPRIO argument takes bitmask of class + ioniceness");

        let tasks_path = format!("/proc/{}/task/", pid.as_raw());
        let dir_iter = std::fs::read_dir(&tasks_path)?;
        for task in dir_iter {
            let task_tid = task?.file_name().to_string_lossy().parse::<u32>()?;

            let value = ioprio_value(IOPRIO_CLASS_BE, ioniceness);
            let ret = libc::syscall(
                libc::SYS_ioprio_set,
                IOPRIO_WHO_PROCESS,
                task_tid,
                value as i32,
            );
            if ret < 0 {
                // Should not fail if failed to change single process' niceness
                eprintln!("Failed to change TID IO niceness");
            }
        }

        Ok(())
    }
}
