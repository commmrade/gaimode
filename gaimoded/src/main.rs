use std::{os::unix::fs::PermissionsExt, path::PathBuf};

mod cpu;
mod listener;
mod optimizer;
mod signals;
mod utils;

#[allow(dead_code)]
struct State {
    path: PathBuf,
    governor: String,
    // TODO: more fields?
}
impl Default for State {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            governor: String::new(),
        }
    }
}

#[allow(dead_code)]
struct ProcessState {
    niceness: i32,
    // #[cfg(target_os = "linux")]
    // ioniceness: i32,
    // TODO: more fields? and linux specific fields
}
impl Default for ProcessState {
    fn default() -> Self {
        Self {
            niceness: 0,
            // #[cfg(target_os = "linux")]
            // ioniceness: 0,
        }
    }
}

#[tokio::main]
async fn main() {
    // if let Err(why) = daemonize() {
    //     eprintln!("Daemonize failed: {}", why);
    //     return;
    // }

    let mut path = std::env::temp_dir();
    path.push(utils::UDS_FILENAME);

    // Temp trick
    if path.exists() {
        std::fs::remove_file(&path).unwrap();
    }

    let listener = tokio::net::UnixListener::bind(&path).expect("UDS creation failed");
    let perms = std::fs::Permissions::from_mode(0x666);
    std::fs::set_permissions(&path, perms).expect("chmod failed");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<utils::Commands>();
    let mut optimizer = optimizer::Optimizer::new();
    let mut listener = listener::UdsListener::new(listener);

    // This looks better than looping in process since now i can make optimizer and listener a static var and lock a mutex when using them
    tokio::spawn(async move {
        loop {
            optimizer.process(&mut rx).await;
        }
    });
    tokio::spawn(async move {
        loop {
            listener.process(&tx).await;
        }
    });

    nix::unistd::unlink(&path).unwrap(); // TODO: Should be done on exit
}
