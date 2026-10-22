//! In-process timing, the counterpart of scripts/bench_inproc_py.py.
//!
//! cargo run --release -p markitdown-core --example bench -- <N> <files...>

use markitdown_core::{Options, StreamInfo, convert};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n: usize = args
        .first()
        .and_then(|a| a.parse().ok())
        .expect("usage: bench <N> <files...>");
    println!("file\tmedian_ms\tmin_ms\tout_chars");
    for f in &args[1..] {
        let info = StreamInfo::from_path(f);
        let out = convert(&std::fs::read(f).unwrap(), &info, &Options::default()).unwrap(); // warm-up
        let mut times: Vec<f64> = (0..n)
            .map(|_| {
                let t = Instant::now();
                let data = std::fs::read(f).unwrap();
                std::hint::black_box(convert(&data, &info, &Options::default()).unwrap());
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        times.sort_by(f64::total_cmp);
        println!(
            "{f}\t{:.3}\t{:.3}\t{}",
            times[n / 2],
            times[0],
            out.chars().count()
        );
    }
}
