use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use wrfcoin_p2p::{message::WrfcoinMessage, scoring::build_peer_scoring_params};

fn sample_messages(count: usize) -> Vec<WrfcoinMessage> {
    (0..count)
        .map(|i| match i % 4 {
            0 => WrfcoinMessage::NewBlock {
                block_bytes: vec![i as u8; 256],
            },
            1 => WrfcoinMessage::NewTransactions {
                tx_bytes: vec![vec![i as u8; 128], vec![(i as u8).wrapping_add(1); 64]],
            },
            2 => WrfcoinMessage::WeatherData {
                station_id: format!("station-{i}"),
                data: vec![i as u8; 128],
                timestamp: 1_700_000_000 + i as i64,
                nonce: None,
                station_signature: String::new(),
                station_pubkey: String::new(),
            },
            _ => WrfcoinMessage::ComputeNodeHeartbeat {
                node_id: format!("node-{i}"),
                capabilities: vec!["wrf".to_string(), "gfs".to_string()],
                region: "us-west".to_string(),
                active_tasks: (i % 5) as u32,
                max_concurrent_tasks: 8,
                observation_density: Some(0.42),
                station_count: Some(120),
            },
        })
        .collect()
}

fn bench_message_serdes(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_serdes");
    for n in [100usize, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::new("count", n), &n, |b, &count| {
            b.iter_batched(
                || sample_messages(count),
                |messages| {
                    let mut bytes_total = 0usize;
                    for message in messages {
                        let encoded = message.to_bytes().expect("serialize message");
                        bytes_total += encoded.len();
                        let decoded =
                            WrfcoinMessage::from_bytes(&encoded).expect("deserialize message");
                        black_box(decoded);
                    }
                    black_box(bytes_total);
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

fn bench_peer_scoring_params(c: &mut Criterion) {
    c.bench_function("peer_scoring_params_build", |b| {
        b.iter(|| {
            let (params, thresholds) = build_peer_scoring_params();
            black_box(params);
            black_box(thresholds);
        });
    });
}

criterion_group!(benches, bench_message_serdes, bench_peer_scoring_params);
criterion_main!(benches);
