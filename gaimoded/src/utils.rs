pub const UDS_FILENAME: &'static str = "gaimoded_sock";

pub enum Commands {
    OptimizeProcess(nix::unistd::Pid),
    ResetProcess(nix::unistd::Pid),
    ResetAll,
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
