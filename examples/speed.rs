//! Native worker for the speed tester (target "rust"). One measurement per process, the
//! result is a single `key=value` line on stdout. Timing mirrors the Python tester:
//! 20 untimed calls, then single calls until MIN_TIME has passed and MIN_ITERS were taken,
//! p50 / p95 with linear interpolation.
//!
//! speed info
//! speed hash_fixed MIN_TIME MIN_ITERS N MSG_LEN
//! speed hash_var   MIN_TIME MIN_ITERS            (u32 LE lengths on stdin)
//! speed single     MIN_TIME MIN_ITERS prng|xof SIZE
//! speed batch      MIN_TIME MIN_ITERS prng|xof N
//! speed multi      prng|xof SIZE WARMUP TIME     (prints `ready`, starts on a line from stdin)

use std::hint::black_box;
use std::io::{BufRead, Read, Write};
use std::process::ExitCode;
use std::time::Instant;

use lustro::{
    hash256, hash256_many_fixed_into, hash256_many_var_into, Hash256, LustroPrng, LustroPrngBatch,
    LustroXof, LustroXofBatch, Seed256, StreamId,
};

const WARMUP_CALLS: usize = 20;
const MSG: &[u8] = b"m0";

fn seed() -> Seed256 {
    let mut s = [0u8; 32];
    for (i, b) in s.iter_mut().enumerate() {
        *b = i as u8;
    }
    Seed256(s)
}

// splitmix64, fixed seed: message content does not depend on the run.
fn fill_data(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        out.extend_from_slice(&z.to_le_bytes());
    }
    out.truncate(len);
    out
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    let k = (sorted.len() - 1) as f64 * q / 100.0;
    let lo = k as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (k - lo as f64)
}

struct Timing {
    p50: f64,
    p95: f64,
}

fn timed_loop(min_time: f64, min_iters: usize, mut f: impl FnMut()) -> Timing {
    for _ in 0..WARMUP_CALLS {
        f();
    }
    let mut times: Vec<f64> = Vec::new();
    let total = Instant::now();
    loop {
        let start = Instant::now();
        f();
        times.push(start.elapsed().as_secs_f64());
        if total.elapsed().as_secs_f64() > min_time && times.len() >= min_iters {
            break;
        }
    }
    times.sort_by(f64::total_cmp);
    Timing {
        p50: percentile(&times, 50.0),
        p95: percentile(&times, 95.0),
    }
}

fn report(t: &Timing, steps: usize, bytes: usize) {
    println!(
        "p50={:.12} p95={:.12} steps={} bytes={}",
        t.p50, t.p95, steps, bytes
    );
}

enum Kind {
    Prng,
    Xof,
}

fn parse_kind(s: &str) -> Result<Kind, String> {
    match s {
        "prng" => Ok(Kind::Prng),
        "xof" => Ok(Kind::Xof),
        _ => Err(format!("unknown stream kind '{s}'")),
    }
}

enum Stream {
    Prng(LustroPrng),
    Xof(LustroXof),
}

impl Stream {
    fn new(kind: &Kind) -> Self {
        match kind {
            Kind::Prng => Stream::Prng(LustroPrng::new(&seed(), StreamId(0))),
            Kind::Xof => Stream::Xof(LustroXof::new(MSG)),
        }
    }

    #[inline]
    fn fill(&mut self, out: &mut [u8]) {
        match self {
            Stream::Prng(s) => s.fill_bytes(out),
            Stream::Xof(s) => s.fill_bytes(out),
        }
    }
}

struct Args {
    it: std::vec::IntoIter<String>,
}

impl Args {
    fn next(&mut self, name: &str) -> Result<String, String> {
        self.it
            .next()
            .ok_or_else(|| format!("missing argument {name}"))
    }

    fn num<T: std::str::FromStr>(&mut self, name: &str) -> Result<T, String> {
        let s = self.next(name)?;
        s.parse()
            .map_err(|_| format!("bad value for {name}: '{s}'"))
    }
}

fn cmd_info() {
    let info = lustro::runtime_info();
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "backend={} threads={} version={} build={}",
        info.backend,
        info.threads,
        env!("CARGO_PKG_VERSION"),
        build
    );
}

fn cmd_hash_fixed(a: &mut Args) -> Result<(), String> {
    let min_time: f64 = a.num("MIN_TIME")?;
    let min_iters: usize = a.num("MIN_ITERS")?;
    let n: usize = a.num("N")?;
    let msg_len: usize = a.num("MSG_LEN")?;
    if n == 0 || msg_len == 0 {
        return Err("N and MSG_LEN must be non-zero".into());
    }
    let data = fill_data(n * msg_len);
    let mut out = vec![Hash256::default(); n];

    hash256_many_fixed_into(&data, msg_len, &mut out);
    if out[0] != hash256(&data[..msg_len]) {
        return Err("hash256_many_fixed_into: row 0 differs from hash256".into());
    }
    let t = timed_loop(min_time, min_iters, || {
        hash256_many_fixed_into(black_box(&data), msg_len, black_box(&mut out));
    });
    black_box(&out);
    report(&t, 0, n * msg_len);
    Ok(())
}

fn cmd_hash_var(a: &mut Args) -> Result<(), String> {
    let min_time: f64 = a.num("MIN_TIME")?;
    let min_iters: usize = a.num("MIN_ITERS")?;
    let mut raw = Vec::new();
    std::io::stdin()
        .lock()
        .read_to_end(&mut raw)
        .map_err(|e| format!("stdin: {e}"))?;
    if raw.is_empty() || raw.len() % 4 != 0 {
        return Err(format!(
            "stdin: {} bytes, expected a non-empty u32 array",
            raw.len()
        ));
    }
    let lens: Vec<usize> = raw
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as usize)
        .collect();
    let n = lens.len();
    let total: usize = lens.iter().sum();
    let blob = fill_data(total);
    let mut messages: Vec<&[u8]> = Vec::with_capacity(n);
    let mut pos = 0;
    for &len in &lens {
        messages.push(&blob[pos..pos + len]);
        pos += len;
    }
    let mut out = vec![Hash256::default(); n];

    hash256_many_var_into(&messages, &mut out);
    for k in [0, n - 1] {
        if out[k] != hash256(messages[k]) {
            return Err(format!(
                "hash256_many_var_into: row {k} differs from hash256"
            ));
        }
    }
    let t = timed_loop(min_time, min_iters, || {
        hash256_many_var_into(black_box(&messages), black_box(&mut out));
    });
    black_box(&out);
    report(&t, 0, total);
    Ok(())
}

fn cmd_single(a: &mut Args) -> Result<(), String> {
    let min_time: f64 = a.num("MIN_TIME")?;
    let min_iters: usize = a.num("MIN_ITERS")?;
    let kind = parse_kind(&a.next("KIND")?)?;
    let size: usize = a.num("SIZE")?;
    let mut stream = Stream::new(&kind);
    let mut buf = vec![0u8; size];
    let t = timed_loop(min_time, min_iters, || {
        stream.fill(black_box(&mut buf));
    });
    black_box(&buf);
    report(&t, 0, size);
    Ok(())
}

fn cmd_batch(a: &mut Args) -> Result<(), String> {
    let min_time: f64 = a.num("MIN_TIME")?;
    let min_iters: usize = a.num("MIN_ITERS")?;
    let kind = parse_kind(&a.next("KIND")?)?;
    let n: usize = a.num("N")?;

    let (steps, t, out_len) = match kind {
        Kind::Prng => {
            let mut batch = LustroPrngBatch::new_range(&seed(), StreamId(0), n);
            let steps = batch.suggested_steps();
            if steps == 0 {
                return Err("suggested_steps returned 0".into());
            }
            let mut out = vec![[0u8; 32]; n * steps];
            let t = timed_loop(min_time, min_iters, || {
                batch.fill_blocks(black_box(&mut out), steps);
            });
            black_box(&out);
            (steps, t, out.len() * 32)
        }
        Kind::Xof => {
            let msgs: Vec<[u8; 8]> = (0..n as u64).map(u64::to_le_bytes).collect();
            let refs: Vec<&[u8]> = msgs.iter().map(|m| m.as_slice()).collect();
            let mut batch = LustroXofBatch::new(&refs);
            let steps = batch.suggested_steps();
            if steps == 0 {
                return Err("suggested_steps returned 0".into());
            }
            let mut out = vec![[0u8; 32]; n * steps];
            let t = timed_loop(min_time, min_iters, || {
                batch.fill_blocks(black_box(&mut out), steps);
            });
            black_box(&out);
            (steps, t, out.len() * 32)
        }
    };
    report(&t, steps, out_len);
    Ok(())
}

fn cmd_multi(a: &mut Args) -> Result<(), String> {
    let kind = parse_kind(&a.next("KIND")?)?;
    let size: usize = a.num("SIZE")?;
    let warmup: f64 = a.num("WARMUP")?;
    let window: f64 = a.num("TIME")?;
    let mut stream = Stream::new(&kind);
    let mut buf = vec![0u8; size];
    let mut call = || stream.fill(black_box(&mut buf));

    let warm_start = Instant::now();
    call();
    while warm_start.elapsed().as_secs_f64() < warmup {
        call();
    }

    println!("ready");
    std::io::stdout()
        .flush()
        .map_err(|e| format!("stdout: {e}"))?;
    let mut line = String::new();
    let got = std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| format!("stdin: {e}"))?;
    if got == 0 || line.trim() != "go" {
        return Err("no start signal".into());
    }

    let mut calls: u64 = 0;
    let start = Instant::now();
    let elapsed = loop {
        call();
        calls += 1;
        let e = start.elapsed().as_secs_f64();
        if e >= window {
            break e;
        }
    };
    println!("calls={calls} elapsed={elapsed:.12}");
    Ok(())
}

fn run() -> Result<(), String> {
    let mut a = Args {
        it: std::env::args().skip(1).collect::<Vec<_>>().into_iter(),
    };
    match a.next("COMMAND")?.as_str() {
        "info" => {
            cmd_info();
            Ok(())
        }
        "hash_fixed" => cmd_hash_fixed(&mut a),
        "hash_var" => cmd_hash_var(&mut a),
        "single" => cmd_single(&mut a),
        "batch" => cmd_batch(&mut a),
        "multi" => cmd_multi(&mut a),
        other => Err(format!("unknown command '{other}'")),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("speed: {e}");
            ExitCode::FAILURE
        }
    }
}
