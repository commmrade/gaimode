use nix::unistd;

#[allow(dead_code)]
fn print_info(name: &str) {
    println!(
        "{} id: {}, gid: {}, sid: {}",
        name,
        unistd::getpid(),
        unistd::getpgid(None).unwrap(),
        unistd::getsid(None).unwrap(),
    );
}

#[allow(dead_code)]
extern "C" fn handle_sig(sig: std::ffi::c_int, act: *const libc::siginfo_t, p: *mut libc::c_void) {}

// TODO: Somehow I should revert settings if i catch a TERM signal
#[allow(dead_code)]
fn setup_signals() {
    unsafe {
        let mut act: libc::sigaction = std::mem::zeroed();
        act.sa_sigaction = handle_sig as usize;
        // act.sa_flags = libc::SA_RESTART; // restart a syscall if it was interrupted by a signal (like waitpid)
        act.sa_sigaction = libc::SIG_IGN; // Ignore siginфt

        /*
        * A child created via fork(2) inherits a copy of its parent's signal
               dispositions.  During an execve(2), the dispositions of handled
               signals are reset to the default; the dispositions of ignored
               signals are left unchanged
        */

        libc::sigaction(
            libc::SIGINT,
            &act as *const libc::sigaction,
            std::ptr::null_mut(),
        );
    }

    // todo!("Find a library to handle this shit (do i need to handle signals");
}
