use nix::{sys::wait::waitpid, unistd};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    net::TcpListener,
    process::exit,
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender},
};
mod gaiproto;
mod signals;
mod utils;

use crate::gaiproto::Gaiproto;

async fn pid_worker(mut rx: UnboundedReceiver<utils::Commands>) {
    let mut processes: HashSet<nix::unistd::Pid> = HashSet::new();

    // TODO: Make a struct for storing old state
    let mut old_cpu_gov = String::new();
    let mut old_niceness = 0;
    let mut is_optimized = false;
    let mut should_reset_all = false;
    loop {
        if let Ok(command) = rx.try_recv() {
            match command {
                utils::Commands::OptimizeProcess(pid) => {
                    println!("Optimizing process {}", pid.as_raw());
                    processes.insert(pid);
                    is_optimized = true;
                }
                utils::Commands::ResetProcess(pid) => {
                    println!("Resetting process {}", pid.as_raw());
                    processes.remove(&pid);
                }
                utils::Commands::ResetAll => {
                    should_reset_all = true;
                }
                _ => {
                    todo!("Handle command");
                }
            }
        }

        processes.retain(|pid| {
            let res = unsafe { nix::libc::kill(pid.as_raw(), 0) };
            if res < 0 && res != nix::libc::EPERM {
                return false;
            }
            return true;
        });
        if is_optimized && processes.is_empty() {
            // TODO: Revert optimizations
            if processes.is_empty() {
                // TODO: In this case all processes are done, so we should jsut reset governor and all those non-pid-bound optimizations
            } else if should_reset_all {
                // TODO: Reset optimizations on all processes + reset non-pid-bound optimizations
                should_reset_all = false;
            }

            is_optimized = false;
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn uds_worker(listener: tokio::net::UnixListener, tx: UnboundedSender<utils::Commands>) {
    loop {
        match listener.accept().await {
            Ok((mut stream, _addr)) => {
                let mut buf: [u8; 2048] = [0u8; 2048];
                stream.read(&mut buf).await.unwrap();

                let packet = Gaiproto::from_bytes(buf.to_vec());
                if let Err(why) = handle_packet(&packet, tx.clone()).await {
                    eprintln!("Handle packet failed: {}", why);
                }
            }
            Err(why) => {
                eprintln!("Accept failed: {}", why);
            }
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
    if path.exists() {
        std::fs::remove_file(&path).unwrap();
    }
    println!("{:?}", path);

    let listener = tokio::net::UnixListener::bind(&path).expect("UDS creation failed");

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<utils::Commands>();

    // tokio::spawn(async move {});
    tokio::spawn(async move {
        pid_worker(rx).await;
    });
    uds_worker(listener, tx).await;

    nix::unistd::unlink(&path).unwrap();
}

async fn handle_packet(pkt: &Gaiproto, tx: UnboundedSender<utils::Commands>) -> anyhow::Result<()> {
    match pkt.kind {
        gaiproto::K_OPTIMIZE_PROCESS => {
            let pid_raw = i32::from_be_bytes(pkt.payload.clone().try_into().unwrap());
            let pid = nix::unistd::Pid::from_raw(pid_raw);
            tx.send(utils::Commands::OptimizeProcess(pid)).unwrap();
        }
        gaiproto::K_RESET_PROCESS => {
            let pid_raw = i32::from_be_bytes(pkt.payload.clone().try_into().unwrap());
            let pid = nix::unistd::Pid::from_raw(pid_raw);
            tx.send(utils::Commands::ResetProcess(pid)).unwrap();
        }
        gaiproto::K_RESET_ALL => {
            tx.send(utils::Commands::ResetAll).unwrap();
        }
        _ => {
            // Ignore
        }
    }
    Ok(())
}
