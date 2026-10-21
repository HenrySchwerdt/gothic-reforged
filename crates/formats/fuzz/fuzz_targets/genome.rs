#![no_main]
use gothic_formats::synth::{Target, run, structured};
use libfuzzer_sys::fuzz_target;

const TARGET: &str = "genome";

fuzz_target!(|data: &[u8]| {
    let t = Target::from_name(TARGET).expect("known target");
    run(t, data);
    run(t, &structured(t, data));
});
