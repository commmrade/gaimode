use std::{os::unix::fs::PermissionsExt, time::Duration};

use clap::{Parser, arg};
use tokio::{
    signal::unix::{Signal, SignalKind},
    task::JoinSet,
};

mod cfg;
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

// Tasks:
// 2. Maybe split optimizer and make it an orchestrator, adapter, but not a god object or typa shit

#[tokio::main]
async fn main() {
    // todo!("- Testing");

    // test 1 (no perf): 958, fps: 22.9
    // test 2 (no perf): 962, fps: 23.0
    // test 3 (gamemode): 952, fps: 22.7
    // test 4 (gamemode): 929, fps: 22.2
    // test 5 (gaimode): 938, fps: 22.4
    // test 6 (gaimode): 920, fps: ...

    let mut sigterm_signal = tokio::signal::unix::signal(SignalKind::terminate())
        .expect("Wasn't able to set up SIGTERM handler"); // systemctl stop service sends this
    let mut sigint_signal = tokio::signal::unix::signal(SignalKind::interrupt())
        .expect("Wasn't able to set up SIGINT handler"); // CTRL+C sends this

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

    let cfg = cfg::get_cfg().unwrap_or_else(|_| cfg::Settings::default());
    let mut optimizer = optimizer::Optimizer::new(cfg);
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
        _ = sigterm_signal.recv() => {
            tracing::info!("Shutting down...");
        }
        _ = sigint_signal.recv() => {
            tracing::info!("Shutting down...");
        }
    }

    tasks_set.shutdown().await;

    if let Err(why) = nix::unistd::unlink(&path) {
        tracing::error!("Wasn't able to unlink UDS file: {}", why);
    }
}
