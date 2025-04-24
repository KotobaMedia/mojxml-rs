use crate::parse::{ParseOptions, ParsedXML};
use crate::reader::{FileData, iter_xml_contents};
use crate::writer::WriterOptions;
use anyhow::Result;
use crossbeam_channel::{bounded, unbounded};
use indicatif::{MultiProgress, ProgressStyle};
use log::{error, info};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::Instant;

pub fn process_files(
    output_path: &Path,
    src_files: Vec<PathBuf>,
    parse_options: ParseOptions,
    write_options: WriterOptions,
) -> Result<usize> {
    let concurrency = num_cpus::get();
    let m = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::stdout_with_hz(2));
    let sty = ProgressStyle::with_template(
        "[{msg}] {elapsed_precise} {bar:40.cyan/blue} {pos:>7}/{len:7}",
    )
    .unwrap()
    .progress_chars("##-");

    let xml_files = Arc::new(AtomicUsize::new(0));

    // XML channels
    let (xml_tx, xml_rx) = unbounded::<PathBuf>();
    let xml_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("unzipping"),
    );
    // Parser channels
    let (parser_tx, parser_rx) = bounded::<FileData>(concurrency);
    let parser_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("XML parse"),
    );
    // Writer channels
    let (writer_tx, writer_rx) = bounded::<ParsedXML>(1);
    let writer_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("FGB write"),
    );

    let start = Instant::now();
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    {
        let xml_pb = xml_pb.clone();
        handles.push(thread::spawn(move || {
            for path in src_files {
                info!("Input file: {}", path.display());
                xml_pb.inc_length(1);
                xml_tx.send(path).unwrap();
            }
        }));
    }
    for i in 0..std::cmp::max(1, concurrency / 4) {
        let xml_rx = xml_rx.clone();
        let parser_tx = parser_tx.clone();
        let xml_pb = xml_pb.clone();
        let parser_pb = parser_pb.clone();
        let xml_files = xml_files.clone();
        handles.push(thread::spawn(move || {
            while let Ok(path) = xml_rx.recv() {
                info!("[ZIP {:>2}] Opening file: {}", i, path.display());
                for item in iter_xml_contents(&path) {
                    match item {
                        Ok(file_data) => {
                            info!(
                                "[ZIP {:>2}] Got XML: {}, size: {}",
                                i,
                                file_data.file_name,
                                file_data.contents.len()
                            );
                            xml_files.fetch_add(1, Ordering::Relaxed);
                            parser_pb.inc_length(1);
                            parser_tx.send(file_data).unwrap();
                        }
                        Err(e) => {
                            error!(
                                "[ZIP {:>2}] Error reading file {}: {}",
                                i,
                                path.display(),
                                e
                            );
                            eprintln!("Error reading file {}: {}", path.display(), e);
                        }
                    }
                }
                xml_pb.inc(1);
            }
        }));
    }
    drop(parser_tx);

    for i in 0..std::cmp::max(2, concurrency - 1) {
        let parser_rx = parser_rx.clone();
        let writer_tx = writer_tx.clone();
        let parser_pb = parser_pb.clone();
        let writer_pb = writer_pb.clone();
        let options = parse_options.clone();
        handles.push(thread::spawn(move || {
            while let Ok(file_data) = parser_rx.recv() {
                info!("[XML {:>2}] Parsing file: {}", i, file_data.file_name);
                let parsed_xml = crate::parse::parse_xml_content(&file_data, &options);
                match parsed_xml {
                    Ok(parsed) => {
                        info!("[XML {:>2}] Parsed file: {}", i, file_data.file_name);
                        writer_pb.inc_length(1);
                        parser_pb.inc(1);
                        writer_tx.send(parsed).unwrap();
                    }
                    Err(e) => {
                        error!(
                            "[XML {:>2}] Error parsing file {}: {}",
                            i, file_data.file_name, e
                        );
                        eprintln!("Error parsing file {}: {}", file_data.file_name, e);
                        parser_pb.inc(1);
                    }
                }
            }
        }));
    }
    drop(writer_tx);

    {
        let output_path = output_path.to_path_buf();
        let writer_pb = writer_pb.clone();
        handles.push(thread::spawn(move || {
            let mut fgb = crate::writer::FGBWriter::new(&output_path, &write_options).unwrap();
            while let Ok(parsed_xml) = writer_rx.recv() {
                info!("[FGB] Adding features from file: {}", parsed_xml.file_name);
                let write_result = fgb.add_xml_features(parsed_xml);
                match write_result {
                    Ok(_) => {
                        writer_pb.inc(1);
                    }
                    Err(e) => {
                        eprintln!("Error writing file {}: {}", output_path.display(), e);
                    }
                }
            }
            info!("[FGB] Starting output file: {}", output_path.display());
            fgb.flush().unwrap();
            info!("[FGB] Finished writing file: {}", output_path.display());
        }));
    }
    let _ = handles
        .into_iter()
        .map(|h| h.join().expect("Thread panicked"))
        .collect::<Vec<_>>();
    let elapsed = start.elapsed();

    xml_pb.finish();
    parser_pb.finish();
    writer_pb.finish();

    println!(
        "\nFinished processing files in {}.{:03}",
        elapsed.as_secs(),
        elapsed.subsec_millis()
    );

    Ok(xml_files.load(Ordering::Relaxed))
}
