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
    let input_file_count = src_files.len();
    let cpu_count = num_cpus::get().max(1);
    let (zip_workers, parse_workers) = worker_counts(cpu_count, input_file_count);
    let parser_queue_capacity = (parse_workers * 2).clamp(4, 32);
    let writer_queue_capacity = (parse_workers * 2).clamp(2, 8);

    let m = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::stdout_with_hz(2));
    let sty = ProgressStyle::with_template(
        "[{msg}] {elapsed_precise} {bar:40.cyan/blue} {pos:>7}/{len:7}",
    )
    .unwrap()
    .progress_chars("##-");

    let xml_document_count = Arc::new(AtomicUsize::new(0));
    let has_features = Arc::new(AtomicBool::new(false));

    let input_files_total = Arc::new(AtomicUsize::new(input_file_count));
    let input_files_done = Arc::new(AtomicUsize::new(0));
    let xml_docs_total = Arc::new(AtomicUsize::new(0));
    let xml_docs_done = Arc::new(AtomicUsize::new(0));
    let write_batches_total = Arc::new(AtomicUsize::new(0));
    let write_batches_done = Arc::new(AtomicUsize::new(0));

    // Input path channels (.xml or .zip)
    let (input_tx, input_rx) = unbounded::<PathBuf>();
    let input_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("input read"),
    );
    // Parser channels
    let (parser_tx, parser_rx) = bounded::<(String, String)>(parser_queue_capacity);
    let parse_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("XML parse "),
    );
    // Writer channels
    let (writer_tx, writer_rx) = bounded::<ParsedXML>(writer_queue_capacity);
    let write_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("  write   "),
    );

    let progress_done = Arc::new(AtomicBool::new(false));
    let progress_handle = {
        let input_pb = input_pb.clone();
        let parse_pb = parse_pb.clone();
        let write_pb = write_pb.clone();
        let input_files_total = input_files_total.clone();
        let input_files_done = input_files_done.clone();
        let xml_docs_total = xml_docs_total.clone();
        let xml_docs_done = xml_docs_done.clone();
        let write_batches_total = write_batches_total.clone();
        let write_batches_done = write_batches_done.clone();
        let progress_done = progress_done.clone();

        thread::spawn(move || {
            let mut last_input_len = usize::MAX;
            let mut last_input_pos = usize::MAX;
            let mut last_parse_len = usize::MAX;
            let mut last_parse_pos = usize::MAX;
            let mut last_write_len = usize::MAX;
            let mut last_write_pos = usize::MAX;

            loop {
                let current_input_len = input_files_total.load(Ordering::Relaxed);
                let current_input_pos = input_files_done
                    .load(Ordering::Relaxed)
                    .min(current_input_len);
                let current_parse_len = xml_docs_total.load(Ordering::Relaxed);
                let current_parse_pos =
                    xml_docs_done.load(Ordering::Relaxed).min(current_parse_len);
                let current_write_len = write_batches_total.load(Ordering::Relaxed);
                let current_write_pos = write_batches_done
                    .load(Ordering::Relaxed)
                    .min(current_write_len);

                if current_input_len != last_input_len {
                    input_pb.set_length(current_input_len as u64);
                    last_input_len = current_input_len;
                }
                if current_input_pos != last_input_pos {
                    input_pb.set_position(current_input_pos as u64);
                    last_input_pos = current_input_pos;
                }
                if current_parse_len != last_parse_len {
                    parse_pb.set_length(current_parse_len as u64);
                    last_parse_len = current_parse_len;
                }
                if current_parse_pos != last_parse_pos {
                    parse_pb.set_position(current_parse_pos as u64);
                    last_parse_pos = current_parse_pos;
                }
                if current_write_len != last_write_len {
                    write_pb.set_length(current_write_len as u64);
                    last_write_len = current_write_len;
                }
                if current_write_pos != last_write_pos {
                    write_pb.set_position(current_write_pos as u64);
                    last_write_pos = current_write_pos;
                }

                if progress_done.load(Ordering::Relaxed) {
                    break;
                }

                thread::sleep(Duration::from_millis(125));
            }

            input_pb.set_length(input_files_total.load(Ordering::Relaxed) as u64);
            input_pb.set_position(input_files_done.load(Ordering::Relaxed) as u64);
            parse_pb.set_length(xml_docs_total.load(Ordering::Relaxed) as u64);
            parse_pb.set_position(xml_docs_done.load(Ordering::Relaxed) as u64);
            write_pb.set_length(write_batches_total.load(Ordering::Relaxed) as u64);
            write_pb.set_position(write_batches_done.load(Ordering::Relaxed) as u64);
        })
    };

    let start = Instant::now();
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    {
        handles.push(thread::spawn(move || {
            for path in src_files {
                debug!("Input file: {}", path.display());
                input_tx.send(path).unwrap();
            }
        }));
    }
    for i in 0..zip_workers {
        let input_rx = input_rx.clone();
        let parser_tx = parser_tx.clone();
        let xml_document_count = xml_document_count.clone();
        let input_files_done = input_files_done.clone();
        let xml_docs_total = xml_docs_total.clone();
        handles.push(thread::spawn(move || {
            while let Ok(path) = input_rx.recv() {
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
                            xml_document_count.fetch_add(1, Ordering::Relaxed);
                            xml_docs_total.fetch_add(1, Ordering::Relaxed);
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
                input_files_done.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    drop(parser_tx);

    for i in 0..parse_workers {
        let parser_rx = parser_rx.clone();
        let writer_tx = writer_tx.clone();
        let xml_docs_done = xml_docs_done.clone();
        let write_batches_total = write_batches_total.clone();
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
                            write_batches_total.fetch_add(1, Ordering::Relaxed);
                            writer_tx.send(parsed).unwrap();
                        }
                    }
                    Err(e) => {
                        error!("[XML {:>2}] Error parsing file {}: {}", i, file_name, e);
                        eprintln!("Error parsing file {}: {}", file_name, e);
                    }
                }
                xml_docs_done.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }
    drop(writer_tx);

    {
        let output_path = output_path.to_path_buf();
        let has_features = has_features.clone();
        let write_batches_done = write_batches_done.clone();
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
                write_batches_done.fetch_add(1, Ordering::Relaxed);
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

    input_pb.finish();
    parse_pb.finish();
    write_pb.finish();

    println!(
        "\nFinished processing files in {}.{:03}",
        elapsed.as_secs(),
        elapsed.subsec_millis()
    );

    if !has_features.load(Ordering::Relaxed) {
        eprintln!("Empty output file: {}", output_path.display());
    }

    Ok(xml_document_count.load(Ordering::Relaxed))
}

fn worker_counts(cpu_count: usize, input_file_count: usize) -> (usize, usize) {
    // Keep one dedicated writer thread and split remaining workers by stage weight.
    let available = cpu_count.saturating_sub(1).max(1);
    let zip_workers = (available / 3).max(1);
    let parse_workers = (available - zip_workers).max(1);

    // Avoid spinning up many workers when only a few input paths are provided.
    let worker_cap = input_file_count.max(1);
    let zip_workers = zip_workers.min(worker_cap);
    let parse_workers = parse_workers.min(worker_cap);

    (zip_workers, parse_workers)
}

#[cfg(test)]
mod tests {
    use super::worker_counts;

    #[test]
    fn caps_workers_by_input_files() {
        let (zip_workers, parse_workers) = worker_counts(32, 1);
        assert_eq!(zip_workers, 1);
        assert_eq!(parse_workers, 1);
    }

    #[test]
    fn keeps_at_least_one_worker_when_input_is_empty() {
        let (zip_workers, parse_workers) = worker_counts(4, 0);
        assert_eq!(zip_workers, 1);
        assert_eq!(parse_workers, 1);
    }
}
