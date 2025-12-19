use std::{os::unix::fs::PermissionsExt, time::Duration};

use clap::{Parser, arg};

mod cpu;
mod io;
mod listener;
mod optimizer;
mod scheduler;
mod signals;
mod utils;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    forked: bool,
}

#[tokio::main]
async fn main() {
    // TODO: Run in systemd/double-fork mode depending on/lack of parameters (systemd is the default way)
    let args = Args::parse();
    if args.forked {
        if let Err(why) = utils::daemonize() {
            eprintln!("Failed to daemonize: {}", why);
            return;
        }
    }

    tracing_subscriber::fmt().pretty().init();

    let mut path = std::env::temp_dir();
    path.push(utils::UDS_FILENAME);

    // Temp trick
    if path.exists() {
        std::fs::remove_file(&path).unwrap();
    }

    let listener = tokio::net::UnixListener::bind(&path).expect("UDS creation failed");
    // let perms = std::fs::Permissions::from_mode(0x666);
    // std::fs::set_permissions(&path, perms).expect("chmod failed");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<utils::Commands>();
    let mut optimizer = optimizer::Optimizer::new();
    let mut listener = listener::UdsListener::new(listener);

    // This looks better than looping in process since now i can make optimizer and listener a static var and lock a mutex when using them
    let optimizer_handle = tokio::spawn(async move {
        loop {
            if let Err(why) = optimizer.process(&mut rx).await {
                tracing::error!("Failed to process optimizations: {}", why);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    let listener_handle = tokio::spawn(async move {
        loop {
            if let Err(why) = listener.process(&tx).await {
                tracing::error!("Listener failed: {}", why);
            }
        }
    });

    // Waits for one of the handlers to finish, if 1 finished, the other one should as well
    tokio::select! {
        _ = optimizer_handle => {}
        _ = listener_handle => {}
    }

    nix::unistd::unlink(&path).unwrap(); // TODO: Should be done on exit
}
