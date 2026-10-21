//! Property tests over every parser: random bytes, structured inputs (valid envelopes around
//! random payloads), and truncations / byte flips of the seeds. A parser must return
//! (Ok or Err) without panicking, within a time bound, and without growing the process
//! beyond a memory ceiling. The libFuzzer targets in `fuzz/` run the same harness much
//! longer, with libFuzzer's own per-input malloc and RSS limits.
use gothic_formats::synth::{Target, run, seeds, structured};
use proptest::prelude::*;
use std::time::{Duration, Instant};

/// Wall time a single parse of a small input may take (debug build).
const MAX_TIME: Duration = Duration::from_secs(5);
/// Peak resident memory of the whole test process.
const MAX_RSS_KIB: u64 = 1024 * 1024;

/// Peak resident set size (`VmHWM`) in KiB; `None` where `/proc` is not available.
fn peak_rss_kib() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with("VmHWM:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn check(target: Target, data: &[u8]) {
    let start = Instant::now();
    run(target, data);
    let took = start.elapsed();
    assert!(
        took < MAX_TIME,
        "{}: {:?} on {} bytes",
        target.name(),
        took,
        data.len()
    );
    if let Some(rss) = peak_rss_kib() {
        assert!(
            rss < MAX_RSS_KIB,
            "{}: process peaked at {} MiB on {} bytes",
            target.name(),
            rss >> 10,
            data.len()
        );
    }
}

fn target() -> impl Strategy<Value = Target> {
    proptest::sample::select(Target::ALL.to_vec())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2048, ..ProptestConfig::default() })]

    #[test]
    fn random_bytes(t in target(), data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        check(t, &data);
    }

    #[test]
    fn structured_inputs(t in target(), data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        check(t, &structured(t, &data));
    }

    #[test]
    fn mutated_seeds(
        t in target(),
        pick in any::<usize>(),
        flips in proptest::collection::vec((any::<usize>(), any::<u8>()), 0..16),
        cut in any::<usize>(),
    ) {
        let all = seeds(t);
        let mut s = all[pick % all.len()].clone();
        if !s.is_empty() {
            for (at, v) in flips {
                let i = at % s.len();
                s[i] = v;
            }
            s.truncate(cut % (s.len() + 1));
        }
        check(t, &s);
    }
}

#[test]
fn every_seed_and_every_prefix() {
    for t in Target::ALL {
        for s in seeds(t) {
            for n in 0..=s.len().min(512) {
                check(t, &s[..n]);
            }
            check(t, &s);
        }
    }
}

#[test]
fn sample_archive_is_valid() {
    let mut pak =
        gothic_formats::PakArchive::read(std::io::Cursor::new(gothic_formats::synth::sample_pak()))
            .expect("sample archive parses");
    assert_eq!(pak.entries.len(), 2);
    assert_eq!(pak.read_payload(0).unwrap(), b"hello stored");
    assert_eq!(
        pak.read_payload(1).unwrap(),
        b"hello compressed hello compressed"
    );
}

#[test]
fn target_names_round_trip() {
    for t in Target::ALL {
        assert_eq!(Target::from_name(t.name()), Some(t));
    }
}

/// A member whose header claims 200 MiB but whose stream is tiny is rejected without
/// reserving the claimed size (found by the `pak` fuzz target as a 700 MB RSS peak).
#[test]
fn zlib_member_cannot_claim_a_huge_size() {
    let mut pak = gothic_formats::synth::sample_pak();
    // Patch the second member's `size` field (u64 after offset and stored_size).
    let needle = 33u64.to_le_bytes();
    let at = pak
        .windows(8)
        .rposition(|w| w == needle)
        .expect("size field of the zlib member");
    pak[at..at + 8].copy_from_slice(&(200u64 << 20).to_le_bytes());
    let mut archive = gothic_formats::PakArchive::read(std::io::Cursor::new(pak)).unwrap();
    let before = peak_rss_kib();
    assert!(archive.read_payload(1).is_err());
    if let (Some(a), Some(b)) = (before, peak_rss_kib()) {
        assert!(b - a < 64 * 1024, "peak grew by {} MiB", (b - a) >> 10);
    }
}
