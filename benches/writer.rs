//! Phase 4 writer throughput benchmarks.
//!
//! These benchmarks measure the throughput of the writer thread
//! under different `FlushPolicy` settings. They use `/dev/null` as
//! the sink to avoid saturating the bench harness's stdout pipe.
//!
//! Run with `cargo bench --bench writer`.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

// Re-implement the writer's batching logic locally so we can
// benchmark it against a `/dev/null` sink without depending on
// the global stdout-emit state. This mirrors the production
// writer_loop's flush policy logic (Phase 4 §FlushPolicy).
struct BenchWriter {
    /// Per-event line size (bytes including '\n').
    line_size: usize,
    /// `max_lines` per batch. 1 == per-line flush.
    max_lines: usize,
    /// `max_bytes` per batch. 0 == no byte threshold.
    max_bytes: usize,
    /// `max_interval` per batch. `Duration::ZERO` == no time threshold.
    max_interval: Duration,
}

impl BenchWriter {
    fn run(&self, n_lines: usize) {
        // Open /dev/null as the sink (a `File` impls `Write`).
        let f = File::options()
            .write(true)
            .open("/dev/null")
            .expect("open /dev/null");
        let mut sink = BufWriter::with_capacity(8 * 1024, f);

        // Pre-build a single payload to reuse (avoids per-line alloc).
        let mut payload = vec![b'x'; self.line_size.saturating_sub(1)];
        payload.push(b'\n');

        let mut batch_bytes: usize = 0;
        let mut batch_lines: usize = 0;
        let mut batch_started = Instant::now();

        for _ in 0..n_lines {
            sink.write_all(&payload).expect("write to /dev/null");
            batch_bytes += self.line_size;
            batch_lines += 1;

            if self.max_lines <= 1 {
                sink.flush().expect("flush");
                batch_bytes = 0;
                batch_lines = 0;
                batch_started = Instant::now();
                continue;
            }

            let bytes_hit = self.max_bytes > 0 && batch_bytes >= self.max_bytes;
            let lines_hit = batch_lines >= self.max_lines;
            if bytes_hit || lines_hit {
                sink.flush().expect("flush");
                batch_bytes = 0;
                batch_lines = 0;
                batch_started = Instant::now();
                continue;
            }
            if self.max_interval > Duration::ZERO && batch_started.elapsed() >= self.max_interval {
                sink.flush().expect("flush");
                batch_bytes = 0;
                batch_lines = 0;
                batch_started = Instant::now();
            }
        }
        sink.flush().expect("final flush");
    }
}

/// Benchmark: throughput at 10k events/s for various policies.
///
/// We use a small line size (32 bytes) which is typical for
/// wide-log events. The /dev/null sink prevents stdout pipe
/// saturation from dominating the measurement.
fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase4_writer_throughput");
    let n_lines = 100_000;

    for line_size in [32usize, 256, 1024] {
        // Per-line flush (Phase 2 behavior, pre-Phase-4).
        let writer = BenchWriter {
            line_size,
            max_lines: 1,
            max_bytes: 0,
            max_interval: Duration::ZERO,
        };
        group.throughput(Throughput::Elements(n_lines as u64));
        group.bench_with_input(
            BenchmarkId::new("per_line", line_size),
            &line_size,
            |b, _| {
                b.iter(|| {
                    writer.run(n_lines);
                });
            },
        );

        // Default batched flush (Phase 4 default).
        let writer = BenchWriter {
            line_size,
            max_lines: 1000,
            max_bytes: 8 * 1024,
            max_interval: Duration::from_millis(100),
        };
        group.throughput(Throughput::Elements(n_lines as u64));
        group.bench_with_input(
            BenchmarkId::new("default_batched", line_size),
            &line_size,
            |b, _| {
                b.iter(|| {
                    writer.run(n_lines);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_throughput);
criterion_main!(benches);
