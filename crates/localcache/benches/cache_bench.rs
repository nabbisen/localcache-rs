use std::hint::black_box;
use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use localcache::{CacheEngine, CacheOptions, ChangeDetectionMode, Codec, ConnectionPool};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BenchPayload {
    label: String,
    score: f64,
    vector: Vec<f32>,
}

impl BenchPayload {
    fn new(index: usize, vector_len: usize) -> Self {
        Self {
            label: format!("document-{index:04}"),
            score: index as f64,
            vector: (0..vector_len)
                .map(|value| (value + index) as f32 * 0.001)
                .collect(),
        }
    }
}

struct EngineFixture {
    _tempdir: TempDir,
    engine: CacheEngine<BenchPayload>,
    paths: Vec<PathBuf>,
}

impl EngineFixture {
    fn populated(entries: usize, vector_len: usize) -> Self {
        let tempdir = tempfile::tempdir().expect("create benchmark tempdir");
        let paths = create_files(&tempdir, entries);
        let engine = CacheEngine::open(CacheOptions {
            database_path: tempdir.path().join("cache.sqlite3"),
            change_detection_mode: ChangeDetectionMode::MetadataOnly,
            codec: Codec::Json,
            ..CacheOptions::default()
        })
        .expect("open benchmark cache");

        let items: Vec<_> = paths
            .iter()
            .enumerate()
            .map(|(index, path)| (path, BenchPayload::new(index, vector_len)))
            .collect();
        let report = engine.batch_set(&items).expect("populate benchmark cache");
        assert_eq!(report.succeeded, entries);
        assert!(report.failed.is_empty());

        Self {
            _tempdir: tempdir,
            engine,
            paths,
        }
    }
}

struct PoolFixture {
    _tempdir: TempDir,
    pool: ConnectionPool<BenchPayload>,
    path: PathBuf,
}

impl PoolFixture {
    fn populated(vector_len: usize) -> Self {
        let tempdir = tempfile::tempdir().expect("create pool benchmark tempdir");
        let path = create_files(&tempdir, 1).remove(0);
        let pool = ConnectionPool::open(CacheOptions {
            database_path: tempdir.path().join("pool.sqlite3"),
            change_detection_mode: ChangeDetectionMode::MetadataOnly,
            codec: Codec::Json,
            ..CacheOptions::default()
        })
        .expect("open benchmark connection pool");
        pool.set(&path, &BenchPayload::new(0, vector_len))
            .expect("populate benchmark connection pool");

        Self {
            _tempdir: tempdir,
            pool,
            path,
        }
    }
}

fn create_files(tempdir: &TempDir, entries: usize) -> Vec<PathBuf> {
    (0..entries)
        .map(|index| {
            let path = tempdir.path().join(format!("document-{index:04}.txt"));
            std::fs::write(&path, format!("benchmark document {index}\n"))
                .expect("write benchmark source file");
            path
        })
        .collect()
}

fn bench_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_set");
    for vector_len in [64, 384, 1536] {
        let fixture = EngineFixture::populated(1, vector_len);
        let payload = BenchPayload::new(1, vector_len);
        group.throughput(Throughput::Elements(vector_len as u64));
        group.bench_with_input(
            BenchmarkId::new("json_vector", vector_len),
            &vector_len,
            |bencher, _| {
                bencher.iter(|| {
                    fixture
                        .engine
                        .set(black_box(&fixture.paths[0]), black_box(&payload))
                        .expect("benchmark set")
                });
            },
        );
    }
    group.finish();
}

fn bench_get(c: &mut Criterion) {
    let fixture = EngineFixture::populated(1, 384);
    c.bench_function("cache_get/warm_json_hit", |bencher| {
        bencher.iter(|| {
            black_box(
                fixture
                    .engine
                    .get(black_box(&fixture.paths[0]))
                    .expect("benchmark get"),
            )
        });
    });
}

fn bench_get_if_fresh(c: &mut Criterion) {
    let fixture = EngineFixture::populated(1, 384);
    c.bench_function("cache_get_if_fresh/metadata_hit", |bencher| {
        bencher.iter(|| {
            black_box(
                fixture
                    .engine
                    .get_if_fresh(black_box(&fixture.paths[0]))
                    .expect("benchmark get_if_fresh"),
            )
        });
    });
}

fn bench_batch_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_batch_set");
    for entries in [10, 100] {
        let fixture = EngineFixture::populated(entries, 64);
        let items: Vec<_> = fixture
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| (path, BenchPayload::new(index + entries, 64)))
            .collect();
        group.throughput(Throughput::Elements(entries as u64));
        group.bench_with_input(
            BenchmarkId::new("json_entries", entries),
            &entries,
            |bencher, _| {
                bencher.iter(|| {
                    let report = fixture
                        .engine
                        .batch_set(black_box(&items))
                        .expect("benchmark batch_set");
                    assert_eq!(report.succeeded, entries);
                    assert!(report.failed.is_empty());
                });
            },
        );
    }
    group.finish();
}

fn bench_query_json(c: &mut Criterion) {
    let fixture = EngineFixture::populated(250, 64);
    c.bench_function("cache_query/json_predicate_sort_limit", |bencher| {
        bencher.iter(|| {
            black_box(
                fixture
                    .engine
                    .query()
                    .field_gt("score", 100.0)
                    .order_by_field("score", false)
                    .limit(25)
                    .run()
                    .expect("benchmark JSON query"),
            )
        });
    });
}

fn bench_status_batch(c: &mut Criterion) {
    let fixture = EngineFixture::populated(100, 64);
    c.bench_function("cache_status_batch/metadata_fresh", |bencher| {
        bencher.iter(|| black_box(fixture.engine.check_status_batch(black_box(&fixture.paths))));
    });
}

fn bench_connection_pool_get(c: &mut Criterion) {
    let fixture = PoolFixture::populated(384);
    c.bench_function("connection_pool/get_warm_json_hit", |bencher| {
        bencher.iter(|| {
            black_box(
                fixture
                    .pool
                    .get(black_box(&fixture.path))
                    .expect("benchmark pooled get"),
            )
        });
    });
}

criterion_group!(
    cache_benches,
    bench_set,
    bench_get,
    bench_get_if_fresh,
    bench_batch_set,
    bench_query_json,
    bench_status_batch,
    bench_connection_pool_get,
);
criterion_main!(cache_benches);
