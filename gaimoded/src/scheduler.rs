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
                return Err(anyhow::anyhow!("Could not setpriority"));
            }
        }
        Ok(())
    }
}
