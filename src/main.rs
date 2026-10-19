use clap::Parser;
use ref_inserter::{DEFAULT_EMPTY_ID, Request, run};
use std::num::ParseIntError;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(about = "Insert a reference WAV into an M8 (FW 6.6) project as sliced chains")]
struct Args {
    /// M8 project file to read
    project: PathBuf,
    /// Reference song, PCM WAV at the project's BPM
    reference_wav: PathBuf,
    /// Local directory the sliced WAV is copied into
    #[arg(long)]
    dest_dir: PathBuf,
    /// The same directory as the M8 sees it, stored in the instrument (e.g. /Samples/ref)
    #[arg(long)]
    m8_dir: String,
    /// New project file to write
    #[arg(long)]
    out: PathBuf,
    /// Hex id of the empty chain and phrase (default FE)
    #[arg(long, value_parser = parse_hex_id)]
    empty_id: Option<u8>,
    /// Replace instrument 7F if it is already used
    #[arg(long)]
    force: bool,
}

fn parse_hex_id(id: &str) -> Result<u8, ParseIntError> {
    u8::from_str_radix(id, 16)
}

fn main() -> ExitCode {
    let args = Args::parse();
    let out = args.out.clone();
    let request = Request {
        project: args.project,
        reference_wav: args.reference_wav,
        dest_dir: args.dest_dir,
        m8_dir: args.m8_dir,
        out: args.out,
        empty_id: args.empty_id.unwrap_or(DEFAULT_EMPTY_ID),
        force: args.force,
    };
    match run(&request) {
        Ok(warnings) => {
            warnings.iter().for_each(|message| eprintln!("{message}"));
            println!("wrote {}", out.display());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
