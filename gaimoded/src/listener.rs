use tokio::{io::AsyncReadExt, sync::mpsc::UnboundedSender};

use crate::utils;

pub struct UdsListener {
    pub listener: tokio::net::UnixListener,
}

impl UdsListener {
    pub fn new(listener: tokio::net::UnixListener) -> UdsListener {
        UdsListener { listener }
    }
    pub async fn process(&mut self, tx: &UnboundedSender<utils::Commands>) {
        match self.listener.accept().await {
            Ok((mut stream, _addr)) => {
                let mut buf: [u8; 2048] = [0u8; 2048];
                stream.read(&mut buf).await.unwrap();

                let packet = gaiproto::Gaiproto::from_bytes(buf.to_vec());
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

async fn handle_packet(
    pkt: &gaiproto::Gaiproto,
    tx: UnboundedSender<utils::Commands>,
) -> anyhow::Result<()> {
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
