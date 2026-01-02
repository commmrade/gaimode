pub const OPTIMIZED_NICE_VALUE: i32 = -10;
pub const DEFAULT_NICE_VALUE: i32 = 0;

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

        if ret == -1 && *libc::__errno_location() != 0 {
            match *libc::__errno_location() {
                libc::ESRCH => return Err(anyhow::anyhow!("no process could be located")),
                libc::EINVAL => return Err(anyhow::anyhow!("Value was not recognized")),
                _ => {
                    return Err(anyhow::anyhow!("Could not get process niceness"));
                }
            }
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
                match *libc::__errno_location() {
                    libc::ESRCH => {
                        return Err(anyhow::anyhow!("process could not be located"));
                    }
                    libc::EINVAL => {
                        return Err(anyhow::anyhow!("value is not recognized"));
                    }
                    libc::EPERM => {
                        return Err(anyhow::anyhow!("insufficient permissions"));
                    }
                    libc::EACCES => {
                        return Err(anyhow::anyhow!("insufficient priviliges"));
                    }
                    _ => {
                        return Err(anyhow::anyhow!("unknown error"));
                    }
                }
            }
        }
        Ok(())
    }
}
