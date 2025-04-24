#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

mod constants;
mod error;
mod parse;
mod processor;
mod reader;
mod writer;

use clap::Parser;
use parse::ParseOptions;
use std::{fs::File, path::PathBuf}; // Import ParseOptions

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

    /// Disable FlatGeobuf index creation (turn this off for large exports).
    #[arg(short, long, default_value_t = false)]
    disable_fgb_index: bool,

    /// Enable logging. Will log to mojxml.log in the current directory.
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.verbose {
        simplelog::WriteLogger::init(
            simplelog::LevelFilter::Info,
            simplelog::Config::default(),
            File::create("mojxml.log")?,
        )?;
    }

    let parse_options = ParseOptions {
        include_arbitrary_crs: cli.arbitrary,
        include_chikugai: cli.chikugai,
    };
    let write_options = writer::WriterOptions {
        write_index: !cli.disable_fgb_index,
    };

    println!("Processing files with options: {:?}...", parse_options);

    let file_count =
        processor::process_files(&cli.dst_file, cli.src_files, parse_options, write_options)?;

    println!("Finished processing {} XML file(s).", file_count);
    println!("Destination: {:?}", cli.dst_file);

    Ok(())
}
