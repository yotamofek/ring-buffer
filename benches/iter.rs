use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ring_buffer::RingBuffer;

fn ring_buffer_skip_benchmark(c: &mut Criterion) {
    let mut ring_buffer: RingBuffer<u32, 16> = RingBuffer::from([0; 16]);
    ring_buffer.pop_first().unwrap();

    c.bench_function("iter_step_by", |b| {
        #[inline(never)]
        fn iter_step_by(buf: RingBuffer<u32, 16>) -> u32 {
            buf.iter().cycle().step_by(103).take(black_box(2048)).sum()
        }

        b.iter(|| iter_step_by(ring_buffer));
    });

    c.bench_function("to_vec", |b| {
        #[inline(never)]
        fn to_vec(buf: &RingBuffer<u32, 16>) {
            let _ = buf.to_vec();
        }

        b.iter(|| {
            to_vec(&black_box(ring_buffer));
        });
    });
}

criterion_group!(benches, ring_buffer_skip_benchmark);
criterion_main!(benches);
