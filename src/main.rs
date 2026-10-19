use clap::{Args, Parser, Subcommand};
use ref_inserter::insert::Options;
use ref_inserter::inspect::inspect;
use ref_inserter::{DEFAULT_CHAIN_PHRASES, DEFAULT_EMPTY_ID, DEFAULT_PHRASE_ID, Request, run};
use std::num::ParseIntError;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    version,
    about = "Insert a reference WAV into an M8 (FW 6.6) project as sliced chains",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    #[command(flatten)]
    insert: Option<InsertArgs>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show instrument 7F and any previous reference insertion in a project
    Inspect(InspectArgs),
}

#[derive(Args)]
struct InsertArgs {
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
    /// Tempo of the reference, if it differs from the project tempo
    #[arg(long)]
    bpm: Option<f32>,
}

#[derive(Args)]
struct InspectArgs {
    /// M8 project file to read
    project: PathBuf,
    /// Hex id of the empty chain and phrase (default FE)
    #[arg(long, value_parser = parse_hex_id)]
    empty_id: Option<u8>,
    /// Hex id of the shared phrase every reference chain plays (default F0)
    #[arg(long, value_parser = parse_hex_id)]
    phrase_id: Option<u8>,
}

fn parse_hex_id(id: &str) -> Result<u8, ParseIntError> {
    u8::from_str_radix(id, 16)
}

fn run_insert(args: InsertArgs) -> Result<(), String> {
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
        bpm: args.bpm,
    };
    let report = run(&request)?;
    report
        .warnings
        .iter()
        .for_each(|message| eprintln!("{message}"));
    report.summary().iter().for_each(|line| println!("{line}"));
    Ok(())
}

fn run_inspect(args: InspectArgs) -> Result<(), String> {
    let options = Options {
        empty: args.empty_id.unwrap_or(DEFAULT_EMPTY_ID),
        phrase: args.phrase_id.unwrap_or(DEFAULT_PHRASE_ID),
        chain_phrases: DEFAULT_CHAIN_PHRASES,
    };
    inspect(&args.project, &options)?
        .iter()
        .for_each(|line| println!("{line}"));
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match (cli.command, cli.insert) {
        (Some(Command::Inspect(args)), _) => run_inspect(args),
        (None, Some(args)) => run_insert(args),
        (None, None) => unreachable!("clap requires the insert arguments without a subcommand"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
