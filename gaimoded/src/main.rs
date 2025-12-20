use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use clap::{Parser, arg};
use tokio::{
    signal::unix::SignalKind,
    task::{JoinError, JoinSet},
};
use tokio_util::task::AbortOnDropHandle;

mod cpu;
mod io;
mod listener;
mod optimizer;
mod scheduler;
mod utils;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    forked: bool,
}

#[tokio::main]
async fn main() {
    let mut signal_stream = tokio::signal::unix::signal(SignalKind::terminate())
        .expect("Wasn't able to set up signal handlers");
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
    let perms = std::fs::Permissions::from_mode(0x666);
    std::fs::set_permissions(&path, perms).expect("chmod failed");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<utils::Commands>();

    let mut optimizer = optimizer::Optimizer::new();
    let mut listener = listener::UdsListener::new(listener);

    let mut tasks_set = JoinSet::new();
    tasks_set.spawn(async move {
        loop {
            if let Err(why) = optimizer.process(&mut rx).await {
                tracing::error!("Failed to process optimizations: {}", why);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    tasks_set.spawn(async move {
        loop {
            if let Err(why) = listener.process(tx.clone()).await {
                tracing::error!("Listener failed: {}", why);
            }
        }
    });
    tokio::select! {
        _ = signal_stream.recv() => {
            tracing::info!("Shutting down...");
        }
    }

    tasks_set.shutdown().await;

    if let Err(why) = nix::unistd::unlink(&path) {
        tracing::error!("Wasn't able to unlink UDS file: {}", why);
    }
}
