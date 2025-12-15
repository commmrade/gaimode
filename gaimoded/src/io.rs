pub const IOPRIO_WHO_PROCESS: i32 = 1;
const IOPRIO_CLASS_SHIFT: i32 = 13;
pub const IOPRIO_PRIO_MASK: i32 = (1 << IOPRIO_CLASS_SHIFT) - 1;
pub const IOPRIO_CLASS_BE: i32 = 2;
pub const OPTIMIZED_IO_NICE_VALUE: i32 = 1;
pub const DEFAULT_IO_NICE_VALUE: i32 = 4;

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

        Ok(ioprio_prio_data(ret as i32))
    }
}

// Optimizes IO performance when game has to load assets
pub fn set_process_io_niceness(pid: nix::unistd::Pid, ioniceness: i32) -> anyhow::Result<()> {
    unsafe {
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
                tracing::error!("Failed to change TID I/O Niceness");
            }
        }

        Ok(())
    }
}
