mod constants;
mod error;
mod parse;
mod processor;
mod reader;
mod writer;

use clap::Parser;
use parse::ParseOptions;
use std::path::PathBuf; // Import ParseOptions

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Output FlatGeobuf file path.
    #[arg(required = true)]
    dst_file: PathBuf,

    /// Input MOJ XML file paths (.xml or .zip).
    #[arg(required = true, num_args = 1..)]
    src_files: Vec<PathBuf>,

    /// Include features from arbitrary coordinate systems ("任意座標系").
    #[arg(short, long, default_value_t = false)]
    arbitrary: bool,

    /// Include features marked as outside district ("地区外") or separate drawing ("別図").
    #[arg(short, long, default_value_t = false)]
    chikugai: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let parse_options = ParseOptions {
        include_arbitrary_crs: cli.arbitrary,
        include_chikugai: cli.chikugai,
    };

    println!("Processing files with options: {:?}...", parse_options);

    let file_count = processor::process_files(&cli.dst_file, cli.src_files, parse_options)?;

    println!("Finished processing {} XML file(s).", file_count);
    println!("Destination: {:?}", cli.dst_file);

    Ok(())
}
