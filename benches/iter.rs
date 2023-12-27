use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ring_buffer::RingBuffer;

fn ring_buffer_skip_benchmark(c: &mut Criterion) {
    let mut ring_buffer: RingBuffer<u32, 16> = RingBuffer::from([0; 16]);
    ring_buffer.pop_first().unwrap();

    c.bench_function("ring_buffer_skip", |b| {
        b.iter(
            #[inline(never)]
            || {
                let _ = black_box(ring_buffer)
                    .iter()
                    .cycle()
                    .step_by(103)
                    .take(black_box(2048))
                    .sum::<u32>();

                let _ = black_box(ring_buffer).to_vec();
            },
        );
    });
}

criterion_group!(benches, ring_buffer_skip_benchmark);
criterion_main!(benches);
