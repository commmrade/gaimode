use std::{
    ffi::{CStr, CString},
    io::Write,
    str::FromStr,
};

use clap::{Parser, Subcommand};
use gaiproto::Gaiproto;
use nix::{sys::wait::waitpid, unistd};

mod dbus_i;

const UDS_FILENAME: &'static str = "gaimoded_sock";

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(arg_required_else_help = true)]
    Run {
        #[arg(value_name = "Binary path")]
        executable: String,
        #[arg(value_name = "Arguments for binary")]
        args: Vec<String>,
    },
    #[command(arg_required_else_help = true)]
    ResetProcess {
        #[arg(value_name = "Process ID")]
        pid: i32,
    },
    ResetAll,
}

fn run(
    bin_name: String,
    args: Vec<String>,
    mut stream: std::os::unix::net::UnixStream,
) -> anyhow::Result<()> {
    match unsafe { unistd::fork() } {
        Ok(unistd::ForkResult::Parent { child }) => {
            let packet = Gaiproto::new(
                (gaiproto::MIN_PACKET_SIZE + std::mem::size_of_val(&child)) as u32,
                gaiproto::K_OPTIMIZE_PROCESS,
                child.as_raw().to_be_bytes().to_vec(),
            );
            let bytes = packet.convert_to_bytes();
            stream.write_all(&bytes)?;

            if let Err(why) = waitpid(child, None) {
                eprintln!("Failed to wait for child: {}", why);
            }
        }
        Ok(unistd::ForkResult::Child) => {
            let mut bin_args = Vec::<CString>::new();
            bin_args.push(CString::from_str(&bin_name)?);
            for arg in args {
                bin_args.push(CString::from_str(&arg).expect("Failed to create CString"));
            }

            let mut bin_name = bin_name.into_bytes();
            bin_name.push(0);

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
    Ok(())
}

fn reset_process(pid: i32, mut stream: std::os::unix::net::UnixStream) -> anyhow::Result<()> {
    let packet = Gaiproto::new(
        (gaiproto::MIN_PACKET_SIZE + std::mem::size_of_val(&pid)) as u32,
        gaiproto::K_RESET_PROCESS,
        pid.to_be_bytes().to_vec(),
    );
    let bytes = packet.convert_to_bytes();
    stream.write_all(&bytes)?;
    Ok(())
}

fn reset_all(mut stream: std::os::unix::net::UnixStream) -> anyhow::Result<()> {
    let packet = Gaiproto::new(
        gaiproto::MIN_PACKET_SIZE as u32,
        gaiproto::K_RESET_ALL,
        Vec::new(),
    );
    let bytes = packet.convert_to_bytes();
    stream.write_all(&bytes)?;
    Ok(())
}

fn main() {
    dbus_i::check_or_spin_up_daemon().unwrap();

    let args = Args::parse();
    let mut path = std::env::temp_dir();
    path.push(UDS_FILENAME);
    let stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
    // TODO: Check if dbus service is running if not start it
    match args.command {
        Commands::Run { executable, args } => {
            if let Err(why) = run(executable, args, stream) {
                eprintln!("Could not run the process: {}", why);
            }
        }
        Commands::ResetProcess { pid } => {
            if let Err(why) = reset_process(pid, stream) {
                eprintln!("Could not reset the process: {}", why);
            }
        }
        #[allow(unused)]
        Commands::ResetAll => {
            if let Err(why) = reset_all(stream) {
                eprintln!("Could not reset processes");
            }
        }
    }
}
