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
