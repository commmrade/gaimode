use nix::unistd;
use tokio::sync::mpsc::UnboundedSender;

use crate::{SHOULD_TERMINATE, utils};

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
#[allow(unused)]
extern "C" fn handle_sig(sig: std::ffi::c_int, act: *const libc::siginfo_t, p: *mut libc::c_void) {
    if sig == libc::SIGTERM {
        tracing::warn!("SIGTERM CAUGHT");
        unsafe {
            SHOULD_TERMINATE.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

#[allow(dead_code)]
pub fn setup_signals() {
    unsafe {
        // TODO: I need to set a flag, which indicates a loop in `Optimizer` to shutdown i suppose
        let mut act: libc::sigaction = std::mem::zeroed();
        act.sa_sigaction = handle_sig as usize;
        libc::sigaction(
            libc::SIGTERM,
            &act as *const libc::sigaction,
            std::ptr::null_mut(),
        );
    }
}
