use std::{
    ffi::{CStr, CString},
    io::{Read, Write},
    net::TcpStream,
    str::FromStr,
    time::Duration,
};

use nix::{sys::wait::waitpid, unistd};

use crate::gaiproto::{Gaiproto, MIN_PACKET_SIZE};
mod gaiproto;
mod uds;

fn test_main() {
    let mut stream = TcpStream::connect("127.0.0.1:8088").unwrap();
    loop {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        input = input.trim().to_owned();

        let packet = Gaiproto::new(
            (MIN_PACKET_SIZE + input.len()) as u32,
            gaiproto::K_RESET_ALL,
            input.into_bytes(),
        );
        let bytes = packet.convert_to_bytes();
        stream.write_all(&bytes).unwrap();
    }
}

const UDS_FILENAME: &'static str = "gaimoded_sock";

fn main() {
    let args: Vec<String> = std::env::args().into_iter().collect();
    if args.len() < 2 {
        eprintln!("You should supply path as second argument: gaimode [application] [flags]");
        return;
    }
    let bin_name = args[1].clone();

    let mut path = std::env::temp_dir();
    path.push(UDS_FILENAME);

    let mut stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
    // TODO: Spin up a daemon if socket not found

    match unsafe { unistd::fork() } {
        Ok(unistd::ForkResult::Parent { child }) => {
            let packet = Gaiproto::new(
                (gaiproto::MIN_PACKET_SIZE + std::mem::size_of_val(&child)) as u32,
                gaiproto::K_OPTIMIZE_PROCESS,
                child.as_raw().to_be_bytes().to_vec(),
            );
            let bytes = packet.convert_to_bytes();
            stream.write_all(&bytes).unwrap();

            // if let Err(why) = waitpid(child, None) {
            //     eprintln!("Failed to wait for child: {}", why);
            // }
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
