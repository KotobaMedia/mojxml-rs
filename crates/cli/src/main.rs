#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

mod processor;
mod writer;

use clap::Parser;
use mojxml_parser::ParseOptions;
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
};
use writer::WriterOptions;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Output file path. The file format is determined by the file extension.
    #[arg(required = true)]
    dst_file: PathBuf,

    /// Input MOJ XML file paths (.xml or .zip).
    #[arg(required = true, num_args = 1..)]
    src_files: Vec<PathBuf>,

    /// Include features from arbitrary coordinate systems (unmapped files) ("任意座標系").
    #[arg(short, long, default_value_t = false)]
    arbitrary: bool,

    /// Include only features from arbitrary coordinate systems ("任意座標系").
    /// This ignores features from globally mapped coordinate systems.
    #[arg(short = 'A', long, default_value_t = false)]
    only_arbitrary: bool,

    /// Include features marked as outside district ("地区外") or separate map ("別図").
    /// You probably don't need this.
    #[arg(short, long, default_value_t = false)]
    chikugai: bool,

    /// Enable logging. Will log to mojxml.log in the current directory.
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    /// Optional temporary directory for unzipping files.
    /// If not specified, the default temporary directory will be used.
    /// Use this option if your /tmp directory doesn't have enough space.
    #[arg(short, long)]
    temp_dir: Option<PathBuf>,

    /// Disable FlatGeobuf spatial index generation.
    /// Has effect only when output extension is `.fgb`.
    #[arg(long, default_value_t = false)]
    fgb_no_index: bool,

    /// Write machine-readable processing metrics to a JSON file.
    #[arg(long, value_name = "FILE")]
    metrics_json: Option<PathBuf>,
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

    if let Some(temp_dir) = &cli.temp_dir {
        fs::create_dir_all(temp_dir)?;
        tempfile::env::override_temp_dir(temp_dir).expect("Failed to set temporary directory");
    }

    let parse_options = ParseOptions {
        include_arbitrary_crs: cli.arbitrary,
        include_only_arbitrary_crs: cli.only_arbitrary,
        include_chikugai: cli.chikugai,
    };
    let writer_options = WriterOptions {
        fgb_write_index: !cli.fgb_no_index,
    };

    println!("Starting processing files...");

    let metrics =
        processor::process_files(&cli.dst_file, cli.src_files, parse_options, writer_options)?;

    if let Some(metrics_path) = &cli.metrics_json {
        if let Some(parent) = metrics_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let mut metrics_file = File::create(metrics_path)?;
        serde_json::to_writer_pretty(&mut metrics_file, &metrics)?;
        writeln!(metrics_file)?;
    }

    println!(
        "Finished processing {} XML file(s).",
        metrics.xml_documents_discovered
    );
    println!("Destination: {}", cli.dst_file.display());

    Ok(())
}
