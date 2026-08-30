use crate::writer::{WriterOptions, make_writer_by_ext_with_options};
use anyhow::Result;
use crossbeam_channel::{bounded, unbounded};
use indicatif::{MultiProgress, ProgressStyle};
use log::{debug, error, info};
use mojxml_parser::{ParseOptions, ParsedXML, parse_xml_content};
use mojxml_reader::iter_xml_contents;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const PARSER_QUEUE_BYTE_BUDGET: usize = 512 * 1024 * 1024;
const WRITER_QUEUE_BYTE_BUDGET: usize = 512 * 1024 * 1024;

struct ByteBudget {
    limit: usize,
    state: Mutex<ByteBudgetState>,
    available: Condvar,
}

#[derive(Default)]
struct ByteBudgetState {
    used: usize,
    next_ticket: u64,
    serving_ticket: u64,
}

impl ByteBudget {
    fn new(limit: usize) -> Arc<Self> {
        assert!(limit > 0, "byte budget must be greater than zero");
        Arc::new(Self {
            limit,
            state: Mutex::new(ByteBudgetState::default()),
            available: Condvar::new(),
        })
    }

    fn reserve(self: &Arc<Self>, bytes: usize) -> BytePermit {
        // An individual document may exceed the budget. Reserve the whole budget
        // for it so that it can still make progress without overlapping another
        // queued or active document.
        let reserved = bytes.max(1).min(self.limit);
        let mut state = self.state.lock().expect("byte budget mutex poisoned");
        let ticket = state.next_ticket;
        state.next_ticket = state
            .next_ticket
            .checked_add(1)
            .expect("byte budget ticket overflow");
        while ticket != state.serving_ticket || state.used.saturating_add(reserved) > self.limit {
            state = self
                .available
                .wait(state)
                .expect("byte budget mutex poisoned");
        }
        state.used += reserved;
        state.serving_ticket += 1;
        drop(state);
        // Wake the next ticket immediately when the remaining budget can admit it.
        self.available.notify_all();

        BytePermit {
            budget: self.clone(),
            reserved,
        }
    }
}

struct BytePermit {
    budget: Arc<ByteBudget>,
    reserved: usize,
}

impl Drop for BytePermit {
    fn drop(&mut self) {
        let mut state = self
            .budget
            .state
            .lock()
            .expect("byte budget mutex poisoned");
        state.used = state
            .used
            .checked_sub(self.reserved)
            .expect("released more bytes than reserved");
        drop(state);
        self.budget.available.notify_all();
    }
}

struct ParserQueueItem {
    file_name: String,
    xml_content: String,
    source_bytes: usize,
    _permit: BytePermit,
}

struct WriterQueueItem {
    parsed: ParsedXML,
    _permit: BytePermit,
}

#[derive(Default)]
struct StageMetrics {
    read_ns: AtomicU64,
    parse_ns: AtomicU64,
    write_ns: AtomicU64,
    flush_ns: AtomicU64,
    parser_queue_wait_ns: AtomicU64,
    writer_queue_wait_ns: AtomicU64,
    input_xml_bytes: AtomicU64,
    input_read_errors: AtomicUsize,
    input_files_without_xml: AtomicUsize,
    parsed_ok_docs: AtomicUsize,
    parse_error_docs: AtomicUsize,
    written_batches: AtomicUsize,
    written_features: AtomicUsize,
    write_error_batches: AtomicUsize,
}

#[derive(Debug, Serialize)]
pub struct ProcessMetrics {
    pub schema_version: u32,
    pub wall_time_seconds: f64,
    pub logical_cpu_count: usize,
    pub input_file_count: usize,
    pub zip_workers: usize,
    pub parse_workers: usize,
    pub input_xml_bytes: u64,
    pub input_read_errors: usize,
    pub input_files_without_xml: usize,
    pub xml_documents_discovered: usize,
    pub xml_documents_parsed_ok: usize,
    pub xml_document_parse_errors: usize,
    pub written_batches: usize,
    pub written_features: usize,
    pub write_error_batches: usize,
    pub output_created: bool,
    pub output_bytes: u64,
    pub read_worker_seconds: f64,
    pub parse_worker_seconds: f64,
    pub writer_add_seconds: f64,
    pub writer_flush_seconds: f64,
    pub parser_queue_wait_seconds: f64,
    pub writer_queue_wait_seconds: f64,
}

pub fn process_files(
    output_path: &Path,
    src_files: Vec<PathBuf>,
    parse_options: ParseOptions,
    writer_options: WriterOptions,
) -> Result<ProcessMetrics> {
    let input_file_count = src_files.len();
    let cpu_count = num_cpus::get().max(1);
    let (zip_workers, parse_workers) = worker_counts(cpu_count, input_file_count);
    let parser_queue_capacity = (parse_workers * 2).clamp(2, 32);
    let writer_queue_capacity = (parse_workers * 2).clamp(2, 16);
    let parser_byte_budget = ByteBudget::new(PARSER_QUEUE_BYTE_BUDGET);
    let writer_byte_budget = ByteBudget::new(WRITER_QUEUE_BYTE_BUDGET);

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
    let stage_metrics = Arc::new(StageMetrics::default());

    // Input path channels (.xml or .zip)
    let (input_tx, input_rx) = unbounded::<PathBuf>();
    let input_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("input read"),
    );
    // Parser channels
    let (parser_tx, parser_rx) = bounded::<ParserQueueItem>(parser_queue_capacity);
    let parse_pb = m.add(
        indicatif::ProgressBar::new(0)
            .with_style(sty.clone())
            .with_message("XML parse "),
    );
    // Writer channels
    let (writer_tx, writer_rx) = bounded::<WriterQueueItem>(writer_queue_capacity);
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
        let stage_metrics = stage_metrics.clone();
        let parser_byte_budget = parser_byte_budget.clone();
        handles.push(thread::spawn(move || {
            while let Ok(path) = input_rx.recv() {
                debug!("[ZIP {:>2}] Opening file: {}", i, path.display());
                let mut xml_iter = iter_xml_contents(&path);
                let mut input_xml_documents = 0usize;
                loop {
                    let read_start = Instant::now();
                    let Some(item) = xml_iter.next() else {
                        break;
                    };
                    stage_metrics
                        .read_ns
                        .fetch_add(duration_to_nanos(read_start.elapsed()), Ordering::Relaxed);

                    match item {
                        Ok(file_data) => {
                            input_xml_documents += 1;
                            let (file_name, xml_content) = file_data;
                            let source_bytes = xml_content.len();
                            stage_metrics
                                .input_xml_bytes
                                .fetch_add(source_bytes as u64, Ordering::Relaxed);
                            debug!(
                                "[ZIP {:>2}] Got XML: {}, size: {}",
                                i, file_name, source_bytes
                            );
                            xml_document_count.fetch_add(1, Ordering::Relaxed);
                            xml_docs_total.fetch_add(1, Ordering::Relaxed);
                            let send_start = Instant::now();
                            let permit = parser_byte_budget.reserve(source_bytes);
                            parser_tx
                                .send(ParserQueueItem {
                                    file_name,
                                    xml_content,
                                    source_bytes,
                                    _permit: permit,
                                })
                                .unwrap();
                            stage_metrics.parser_queue_wait_ns.fetch_add(
                                duration_to_nanos(send_start.elapsed()),
                                Ordering::Relaxed,
                            );
                        }
                        Err(e) => {
                            stage_metrics
                                .input_read_errors
                                .fetch_add(1, Ordering::Relaxed);
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
                if input_xml_documents == 0 {
                    stage_metrics
                        .input_files_without_xml
                        .fetch_add(1, Ordering::Relaxed);
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
        let stage_metrics = stage_metrics.clone();
        let writer_byte_budget = writer_byte_budget.clone();
        handles.push(thread::spawn(move || {
            while let Ok(ParserQueueItem {
                file_name,
                xml_content,
                source_bytes,
                _permit: parser_permit,
            }) = parser_rx.recv()
            {
                debug!("[XML {:>2}] Parsing file: {}", i, file_name);
                let parse_start = Instant::now();
                let parsed_xml = parse_xml_content(&file_name, &xml_content, &options);
                stage_metrics
                    .parse_ns
                    .fetch_add(duration_to_nanos(parse_start.elapsed()), Ordering::Relaxed);
                drop(xml_content);
                drop(parser_permit);
                match parsed_xml {
                    Ok(parsed) => {
                        stage_metrics.parsed_ok_docs.fetch_add(1, Ordering::Relaxed);
                        debug!("[XML {:>2}] Parsed file: {}", i, file_name);
                        if parsed.features.is_empty() {
                            debug!(
                                "[XML {:>2}] No features in {}, skipping writer queue",
                                i, file_name
                            );
                        } else {
                            write_batches_total.fetch_add(1, Ordering::Relaxed);
                            let send_start = Instant::now();
                            let permit = writer_byte_budget.reserve(source_bytes);
                            writer_tx
                                .send(WriterQueueItem {
                                    parsed,
                                    _permit: permit,
                                })
                                .unwrap();
                            stage_metrics.writer_queue_wait_ns.fetch_add(
                                duration_to_nanos(send_start.elapsed()),
                                Ordering::Relaxed,
                            );
                        }
                    }
                    Err(e) => {
                        stage_metrics
                            .parse_error_docs
                            .fetch_add(1, Ordering::Relaxed);
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
        let stage_metrics = stage_metrics.clone();
        handles.push(thread::spawn(move || {
            let mut writer = make_writer_by_ext_with_options(&output_path, writer_options).unwrap();
            while let Ok(WriterQueueItem {
                parsed: parsed_xml,
                _permit: writer_permit,
            }) = writer_rx.recv()
            {
                debug!("[OUT] Adding features from file: {}", parsed_xml.file_name);
                let features_in_batch = parsed_xml.features.len();
                let write_start = Instant::now();
                let write_result = writer.add_xml_features(parsed_xml);
                stage_metrics
                    .write_ns
                    .fetch_add(duration_to_nanos(write_start.elapsed()), Ordering::Relaxed);
                match write_result {
                    Ok(_) => {
                        stage_metrics
                            .written_batches
                            .fetch_add(1, Ordering::Relaxed);
                        stage_metrics
                            .written_features
                            .fetch_add(features_in_batch, Ordering::Relaxed);
                    }
                    Err(e) => {
                        stage_metrics
                            .write_error_batches
                            .fetch_add(1, Ordering::Relaxed);
                        eprintln!("Error writing file {}: {}", output_path.display(), e);
                    }
                }
                write_batches_done.fetch_add(1, Ordering::Relaxed);
                drop(writer_permit);
            }
            debug!("[OUT] Starting output file: {}", output_path.display());
            let flush_start = Instant::now();
            let created_file = writer.flush().unwrap();
            stage_metrics
                .flush_ns
                .fetch_add(duration_to_nanos(flush_start.elapsed()), Ordering::Relaxed);
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

    let output_created = has_features.load(Ordering::Relaxed);
    if !output_created {
        eprintln!("Empty output file: {}", output_path.display());
    }

    let metrics = ProcessMetrics {
        schema_version: 1,
        wall_time_seconds: elapsed.as_secs_f64(),
        logical_cpu_count: cpu_count,
        input_file_count,
        zip_workers,
        parse_workers,
        input_xml_bytes: stage_metrics.input_xml_bytes.load(Ordering::Relaxed),
        input_read_errors: stage_metrics.input_read_errors.load(Ordering::Relaxed),
        input_files_without_xml: stage_metrics
            .input_files_without_xml
            .load(Ordering::Relaxed),
        xml_documents_discovered: xml_document_count.load(Ordering::Relaxed),
        xml_documents_parsed_ok: stage_metrics.parsed_ok_docs.load(Ordering::Relaxed),
        xml_document_parse_errors: stage_metrics.parse_error_docs.load(Ordering::Relaxed),
        written_batches: stage_metrics.written_batches.load(Ordering::Relaxed),
        written_features: stage_metrics.written_features.load(Ordering::Relaxed),
        write_error_batches: stage_metrics.write_error_batches.load(Ordering::Relaxed),
        output_created,
        output_bytes: std::fs::metadata(output_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        read_worker_seconds: nanos_to_seconds(stage_metrics.read_ns.load(Ordering::Relaxed)),
        parse_worker_seconds: nanos_to_seconds(stage_metrics.parse_ns.load(Ordering::Relaxed)),
        writer_add_seconds: nanos_to_seconds(stage_metrics.write_ns.load(Ordering::Relaxed)),
        writer_flush_seconds: nanos_to_seconds(stage_metrics.flush_ns.load(Ordering::Relaxed)),
        parser_queue_wait_seconds: nanos_to_seconds(
            stage_metrics.parser_queue_wait_ns.load(Ordering::Relaxed),
        ),
        writer_queue_wait_seconds: nanos_to_seconds(
            stage_metrics.writer_queue_wait_ns.load(Ordering::Relaxed),
        ),
    };

    emit_stage_metrics(&metrics);

    Ok(metrics)
}

fn emit_stage_metrics(metrics: &ProcessMetrics) {
    if !log::log_enabled!(log::Level::Info) {
        return;
    }

    let wall_clock = Duration::from_secs_f64(metrics.wall_time_seconds);

    info!("Stage timing summary (aggregate worker time):");
    info!(
        "  read/decompress: {} ({:.1}% of wall)",
        format_seconds(metrics.read_worker_seconds),
        percent_of_wall(metrics.read_worker_seconds, wall_clock)
    );
    info!(
        "  parse XML:       {} ({:.1}% of wall)",
        format_seconds(metrics.parse_worker_seconds),
        percent_of_wall(metrics.parse_worker_seconds, wall_clock)
    );
    info!(
        "  writer add:      {} ({:.1}% of wall)",
        format_seconds(metrics.writer_add_seconds),
        percent_of_wall(metrics.writer_add_seconds, wall_clock)
    );
    info!(
        "  writer flush:    {} ({:.1}% of wall)",
        format_seconds(metrics.writer_flush_seconds),
        percent_of_wall(metrics.writer_flush_seconds, wall_clock)
    );
    info!(
        "  parser queue wait: {} ({:.1}% of wall)",
        format_seconds(metrics.parser_queue_wait_seconds),
        percent_of_wall(metrics.parser_queue_wait_seconds, wall_clock)
    );
    info!(
        "  writer queue wait: {} ({:.1}% of wall)",
        format_seconds(metrics.writer_queue_wait_seconds),
        percent_of_wall(metrics.writer_queue_wait_seconds, wall_clock)
    );

    let input_mib = bytes_to_mib(metrics.input_xml_bytes);
    let wall_secs = wall_clock.as_secs_f64();
    let end_to_end_mib_per_s = if wall_secs > 0.0 {
        input_mib / wall_secs
    } else {
        0.0
    };
    let parse_secs = metrics.parse_worker_seconds;
    let parse_mib_per_s = if parse_secs > 0.0 {
        input_mib / parse_secs
    } else {
        0.0
    };

    info!("Stage throughput summary:");
    info!("  Input read errors: {}", metrics.input_read_errors);
    info!(
        "  Input files without XML: {}",
        metrics.input_files_without_xml
    );
    info!(
        "  XML documents discovered: {}",
        metrics.xml_documents_discovered
    );
    info!(
        "  XML documents parsed OK: {}",
        metrics.xml_documents_parsed_ok
    );
    info!(
        "  XML documents parse errors: {}",
        metrics.xml_document_parse_errors
    );
    info!("  Input XML bytes: {:.2} MiB", input_mib);
    info!(
        "  Parse throughput (aggregate): {:.2} MiB/s",
        parse_mib_per_s
    );
    info!("  End-to-end throughput: {:.2} MiB/s", end_to_end_mib_per_s);
    info!("  Written batches: {}", metrics.written_batches);
    info!("  Written features: {}", metrics.written_features);
    info!("  Writer batch errors: {}", metrics.write_error_batches);
}

#[inline]
fn duration_to_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

#[inline]
fn nanos_to_seconds(nanos: u64) -> f64 {
    Duration::from_nanos(nanos).as_secs_f64()
}

#[inline]
fn format_seconds(seconds: f64) -> String {
    format!("{seconds:.3}s")
}

#[inline]
fn percent_of_wall(stage_seconds: f64, wall_clock: Duration) -> f64 {
    let wall_seconds = wall_clock.as_secs_f64();
    if wall_seconds == 0.0 {
        return 0.0;
    }
    (stage_seconds / wall_seconds) * 100.0
}

#[inline]
fn bytes_to_mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn worker_counts(cpu_count: usize, input_file_count: usize) -> (usize, usize) {
    // Keep one dedicated writer thread. Reading can only parallelize across input
    // paths, but parsing can parallelize XML documents discovered inside one ZIP.
    let available = cpu_count.saturating_sub(1).max(1);
    let worker_cap = input_file_count.max(1);
    let zip_workers = (available / 3).max(1).min(worker_cap);
    let parse_workers = (available - zip_workers).max(1);

    (zip_workers, parse_workers)
}

#[cfg(test)]
mod tests {
    use super::{ByteBudget, worker_counts};

    #[test]
    fn caps_only_zip_workers_by_input_files() {
        let (zip_workers, parse_workers) = worker_counts(32, 1);
        assert_eq!(zip_workers, 1);
        assert_eq!(parse_workers, 30);
    }

    #[test]
    fn keeps_at_least_one_worker_when_input_is_empty() {
        let (zip_workers, parse_workers) = worker_counts(4, 0);
        assert_eq!(zip_workers, 1);
        assert_eq!(parse_workers, 2);
    }

    #[test]
    fn splits_workers_when_there_are_many_input_files() {
        let (zip_workers, parse_workers) = worker_counts(10, 100);
        assert_eq!(zip_workers, 3);
        assert_eq!(parse_workers, 6);
    }

    #[test]
    fn byte_budget_releases_reserved_capacity() {
        let budget = ByteBudget::new(100);
        let permit = budget.reserve(40);
        assert_eq!(budget.state.lock().unwrap().used, 40);

        drop(permit);
        assert_eq!(budget.state.lock().unwrap().used, 0);
    }

    #[test]
    fn oversized_item_reserves_the_whole_budget() {
        let budget = ByteBudget::new(100);
        let permit = budget.reserve(1_000);
        assert_eq!(budget.state.lock().unwrap().used, 100);

        drop(permit);
        assert_eq!(budget.state.lock().unwrap().used, 0);
    }
}
