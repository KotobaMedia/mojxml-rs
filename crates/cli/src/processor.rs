use crate::writer::make_writer_by_ext;
use anyhow::Result;
use crossbeam_channel::{bounded, unbounded};
use indicatif::{MultiProgress, ProgressStyle};
use log::{debug, error};
use mojxml_parser::{ParseOptions, ParsedXML, parse_xml_content};
use mojxml_reader::iter_xml_contents;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub fn process_files(
    output_path: &Path,
    src_files: Vec<PathBuf>,
    parse_options: ParseOptions,
) -> Result<usize> {
    let cpu_count = num_cpus::get().max(1);
    let (zip_workers, parse_workers) = worker_counts(cpu_count);
    let parser_queue_capacity = (parse_workers * 2).clamp(4, 32);
    let writer_queue_capacity = (parse_workers * 2).clamp(2, 8);

    let m = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::stdout_with_hz(2));
    let sty = ProgressStyle::with_template(
        "[{msg}] {elapsed_precise} {bar:40.cyan/blue} {pos:>7}/{len:7}",
    )
    .unwrap()
    .progress_chars("##-");

    let xml_files = Arc::new(AtomicUsize::new(0));
    let has_features = Arc::new(AtomicBool::new(false));

    let xml_total = Arc::new(AtomicUsize::new(src_files.len()));
    let xml_done = Arc::new(AtomicUsize::new(0));
    let parser_total = Arc::new(AtomicUsize::new(0));
    let parser_done = Arc::new(AtomicUsize::new(0));
    let writer_total = Arc::new(AtomicUsize::new(0));
    let writer_done = Arc::new(AtomicUsize::new(0));

    // XML channels
    let (xml_tx, xml_rx) = unbounded::<PathBuf>();
    let xml_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("unzipping"),
    );
    // Parser channels
    let (parser_tx, parser_rx) = bounded::<(String, String)>(parser_queue_capacity);
    let parser_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("XML parse"),
    );
    // Writer channels
    let (writer_tx, writer_rx) = bounded::<ParsedXML>(writer_queue_capacity);
    let writer_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("  write  "),
    );

    let progress_done = Arc::new(AtomicBool::new(false));
    let progress_handle = {
        let xml_pb = xml_pb.clone();
        let parser_pb = parser_pb.clone();
        let writer_pb = writer_pb.clone();
        let xml_total = xml_total.clone();
        let xml_done = xml_done.clone();
        let parser_total = parser_total.clone();
        let parser_done = parser_done.clone();
        let writer_total = writer_total.clone();
        let writer_done = writer_done.clone();
        let progress_done = progress_done.clone();

        thread::spawn(move || {
            let mut last_xml_len = usize::MAX;
            let mut last_xml_pos = usize::MAX;
            let mut last_parser_len = usize::MAX;
            let mut last_parser_pos = usize::MAX;
            let mut last_writer_len = usize::MAX;
            let mut last_writer_pos = usize::MAX;

            loop {
                let current_xml_len = xml_total.load(Ordering::Relaxed);
                let current_xml_pos = xml_done.load(Ordering::Relaxed).min(current_xml_len);
                let current_parser_len = parser_total.load(Ordering::Relaxed);
                let current_parser_pos =
                    parser_done.load(Ordering::Relaxed).min(current_parser_len);
                let current_writer_len = writer_total.load(Ordering::Relaxed);
                let current_writer_pos =
                    writer_done.load(Ordering::Relaxed).min(current_writer_len);

                if current_xml_len != last_xml_len {
                    xml_pb.set_length(current_xml_len as u64);
                    last_xml_len = current_xml_len;
                }
                if current_xml_pos != last_xml_pos {
                    xml_pb.set_position(current_xml_pos as u64);
                    last_xml_pos = current_xml_pos;
                }
                if current_parser_len != last_parser_len {
                    parser_pb.set_length(current_parser_len as u64);
                    last_parser_len = current_parser_len;
                }
                if current_parser_pos != last_parser_pos {
                    parser_pb.set_position(current_parser_pos as u64);
                    last_parser_pos = current_parser_pos;
                }
                if current_writer_len != last_writer_len {
                    writer_pb.set_length(current_writer_len as u64);
                    last_writer_len = current_writer_len;
                }
                if current_writer_pos != last_writer_pos {
                    writer_pb.set_position(current_writer_pos as u64);
                    last_writer_pos = current_writer_pos;
                }

                if progress_done.load(Ordering::Relaxed) {
                    break;
                }

                thread::sleep(Duration::from_millis(125));
            }

            xml_pb.set_length(xml_total.load(Ordering::Relaxed) as u64);
            xml_pb.set_position(xml_done.load(Ordering::Relaxed) as u64);
            parser_pb.set_length(parser_total.load(Ordering::Relaxed) as u64);
            parser_pb.set_position(parser_done.load(Ordering::Relaxed) as u64);
            writer_pb.set_length(writer_total.load(Ordering::Relaxed) as u64);
            writer_pb.set_position(writer_done.load(Ordering::Relaxed) as u64);
        })
    };

    let start = Instant::now();
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    {
        handles.push(thread::spawn(move || {
            for path in src_files {
                debug!("Input file: {}", path.display());
                xml_tx.send(path).unwrap();
            }
        }));
    }
    for i in 0..zip_workers {
        let xml_rx = xml_rx.clone();
        let parser_tx = parser_tx.clone();
        let xml_files = xml_files.clone();
        let xml_done = xml_done.clone();
        let parser_total = parser_total.clone();
        handles.push(thread::spawn(move || {
            while let Ok(path) = xml_rx.recv() {
                debug!("[ZIP {:>2}] Opening file: {}", i, path.display());
                for item in iter_xml_contents(&path) {
                    match item {
                        Ok(file_data) => {
                            debug!(
                                "[ZIP {:>2}] Got XML: {}, size: {}",
                                i,
                                file_data.0,
                                file_data.1.len()
                            );
                            xml_files.fetch_add(1, Ordering::Relaxed);
                            parser_total.fetch_add(1, Ordering::Relaxed);
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
                xml_done.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    drop(parser_tx);

    for i in 0..parse_workers {
        let parser_rx = parser_rx.clone();
        let writer_tx = writer_tx.clone();
        let parser_done = parser_done.clone();
        let writer_total = writer_total.clone();
        let options = parse_options.clone();
        handles.push(thread::spawn(move || {
            while let Ok((file_name, xml_content)) = parser_rx.recv() {
                debug!("[XML {:>2}] Parsing file: {}", i, file_name);
                let parsed_xml = parse_xml_content(&file_name, &xml_content, &options);
                match parsed_xml {
                    Ok(parsed) => {
                        debug!("[XML {:>2}] Parsed file: {}", i, file_name);
                        if parsed.features.is_empty() {
                            debug!(
                                "[XML {:>2}] No features in {}, skipping writer queue",
                                i, file_name
                            );
                        } else {
                            writer_total.fetch_add(1, Ordering::Relaxed);
                            writer_tx.send(parsed).unwrap();
                        }
                    }
                    Err(e) => {
                        error!("[XML {:>2}] Error parsing file {}: {}", i, file_name, e);
                        eprintln!("Error parsing file {}: {}", file_name, e);
                    }
                }
                parser_done.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    drop(writer_tx);

    {
        let output_path = output_path.to_path_buf();
        let has_features = has_features.clone();
        let writer_done = writer_done.clone();
        handles.push(thread::spawn(move || {
            let mut writer = make_writer_by_ext(&output_path).unwrap();
            while let Ok(parsed_xml) = writer_rx.recv() {
                debug!("[OUT] Adding features from file: {}", parsed_xml.file_name);
                let write_result = writer.add_xml_features(parsed_xml);
                match write_result {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("Error writing file {}: {}", output_path.display(), e);
                    }
                }
                writer_done.fetch_add(1, Ordering::Relaxed);
            }
            debug!("[OUT] Starting output file: {}", output_path.display());
            let created_file = writer.flush().unwrap();
            if !created_file {
                debug!("[OUT] No features written");
            } else {
                debug!("[OUT] Finished writing file: {}", output_path.display());
                has_features.store(true, Ordering::Relaxed);
            }
        }));
    }

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    progress_done.store(true, Ordering::Relaxed);
    progress_handle.join().expect("Progress thread panicked");

    let elapsed = start.elapsed();

    xml_pb.finish();
    parser_pb.finish();
    writer_pb.finish();

    println!(
        "\nFinished processing files in {}.{:03}",
        elapsed.as_secs(),
        elapsed.subsec_millis()
    );

    if !has_features.load(Ordering::Relaxed) {
        eprintln!("Empty output file: {}", output_path.display());
    }

    Ok(xml_files.load(Ordering::Relaxed))
}

fn worker_counts(cpu_count: usize) -> (usize, usize) {
    // Keep one dedicated writer thread and split remaining workers by stage weight.
    let available = cpu_count.saturating_sub(1).max(1);
    let zip_workers = (available / 3).max(1);
    let parse_workers = (available - zip_workers).max(1);
    (zip_workers, parse_workers)
}
