use std::time::Duration;

use nix::{sys::wait::waitpid, unistd};

fn print_info(name: &str) {
    println!(
        "{} id: {}, gid: {}, sid: {}",
        name,
        unistd::getpid(),
        unistd::getpgid(None).unwrap(),
        unistd::getsid(None).unwrap(),
    );
}

extern "C" fn handle_sig(sig: std::ffi::c_int, act: *const libc::siginfo_t, p: *mut libc::c_void) {}

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

fn main() {
    println!("Start");

    match unsafe { unistd::fork() } {
        Ok(unistd::ForkResult::Parent { child }) => {
            print_info("parent");

            if let Err(why) = waitpid(child, None) {
                eprintln!("Failed to waitpid: {}", why);
            }
        }
        Ok(unistd::ForkResult::Child) => {
            if let Err(why) = unistd::setsid() {
                eprintln!("setsid failed: {}", why);
            }

            print_info("Child after SID");

            match unsafe { unistd::fork() } {
                Ok(unistd::ForkResult::Child) => {
                    print_info("Child daemon");

                    loop {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                Ok(unistd::ForkResult::Parent { .. }) => {}
                Err(why) => {
                    eprintln!("Fork failed: {}", why);
                }
            }
        }
        Err(why) => {
            eprintln!("Fork failed: {}", why);
        }
    }
}
