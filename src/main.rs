use clap::Parser;
use ref_inserter::{DEFAULT_CHAIN_PHRASES, DEFAULT_EMPTY_ID, DEFAULT_PHRASE_ID, Request, run};
use std::num::ParseIntError;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    version,
    about = "Insert a reference WAV into an M8 (FW 6.6) project as sliced chains"
)]
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
    /// Hex id of the shared phrase every reference chain plays (default F0)
    #[arg(long, value_parser = parse_hex_id)]
    phrase_id: Option<u8>,
    /// Phrases per chain: 4, 8, 12 or 16, so 1 to 4 slices share a chain (default 4)
    #[arg(long)]
    chain_phrases: Option<usize>,
    /// Replace a previous insertion, a used instrument 7F, and existing output files
    #[arg(long)]
    force: bool,
    /// Validate and print what would change, without writing any file
    #[arg(long)]
    dry_run: bool,
}

fn parse_hex_id(id: &str) -> Result<u8, ParseIntError> {
    u8::from_str_radix(id, 16)
}

fn main() -> ExitCode {
    let args = Args::parse();
    let request = Request {
        project: args.project,
        reference_wav: args.reference_wav,
        dest_dir: args.dest_dir,
        m8_dir: args.m8_dir,
        out: args.out,
        empty_id: args.empty_id.unwrap_or(DEFAULT_EMPTY_ID),
        phrase_id: args.phrase_id.unwrap_or(DEFAULT_PHRASE_ID),
        chain_phrases: args.chain_phrases.unwrap_or(DEFAULT_CHAIN_PHRASES),
        force: args.force,
        dry_run: args.dry_run,
    };
    match run(&request) {
        Ok(report) => {
            report
                .warnings
                .iter()
                .for_each(|message| eprintln!("{message}"));
            report.summary().iter().for_each(|line| println!("{line}"));
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
