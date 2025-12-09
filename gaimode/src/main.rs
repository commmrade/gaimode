use std::{
    ffi::{CStr, CString},
    str::FromStr,
};

use nix::{libc, sys::wait::waitpid, unistd};

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
    setup_signals();

    let args: Vec<String> = std::env::args().into_iter().collect();
    if args.len() < 2 {
        eprintln!("You should supply path as second argument: gaimode [application] [flags]");
        return;
    }
    let bin_name = args[1].clone();

    match unsafe { unistd::fork() } {
        Ok(unistd::ForkResult::Parent { child }) => {
            // TODO: Optimization here on child PID.
            if let Err(why) = waitpid(child, None) {
                eprintln!("Failed to wait for child: {}", why);
            }

            println!("Process has been finished");
        }
        Ok(unistd::ForkResult::Child) => {
            let mut bin_name = bin_name.into_bytes();
            bin_name.push(0);

            let mut bin_args = Vec::<CString>::new();
            for arg in &args[1..] {
                bin_args.push(CString::from_str(&arg).expect("Failed to create CString"));
            }

            let bin_name = CStr::from_bytes_with_nul(&bin_name).expect("Failed to create CStr");
            let err = unistd::execvp(bin_name, &bin_args).err().unwrap(); // Safe: because returns only when there is an error
            println!(
                "Could not start {} because {}",
                bin_name.to_string_lossy(),
                err
            );
        }
        Err(why) => {
            eprintln!("Failed to fork: {}", why);
        }
    }
}
