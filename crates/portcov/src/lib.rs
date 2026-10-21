//! Wiring-coverage counters for the ported original functions.
//!
//! `gothic_portcov::hit!("name")` is inserted (mechanically, by `scripts/portcov/instrument.py`)
//! at the top of every function whose doc comment says `Port of <Name> (<Dll>:<addr>)`. Without
//! the `enabled` feature the macro expands to nothing, so release builds pay nothing. With it,
//! every call site owns a static counter that registers itself on its first hit; a background
//! thread (and [`dump`]) writes `{"<file>#<name>": count, ...}` to the file named by the
//! `GOTHIC_PORTCOV_OUT` environment variable (default `.local/portcov/out.json`).
//! `scripts/portcov/report.py` merges the dumps of all QA scenarios with the static manifest.
//!
//! [`hit_dyn`] counts by runtime name (used for the script state machine, whose `ZS_*` / `PS_*`
//! bodies are data tables and cannot carry a macro).

/// Count one call of the ported function `$name` (key = file + name). No-op when disabled.
#[cfg(feature = "enabled")]
#[macro_export]
macro_rules! hit {
    ($name:expr) => {{
        static COUNTER: ::std::sync::atomic::AtomicU64 = ::std::sync::atomic::AtomicU64::new(0);
        static REGISTER: ::std::sync::Once = ::std::sync::Once::new();
        REGISTER.call_once(|| $crate::register(concat!(file!(), "#", $name), &COUNTER));
        COUNTER.fetch_add(1, ::std::sync::atomic::Ordering::Relaxed);
    }};
}

/// Disabled: expands to nothing.
#[cfg(not(feature = "enabled"))]
#[macro_export]
macro_rules! hit {
    ($name:expr) => {};
}

/// Count by a formatted runtime name, e.g. `hitf!("state:{:?}", s)`. No-op when disabled.
#[cfg(feature = "enabled")]
#[macro_export]
macro_rules! hitf {
    ($($arg:tt)*) => {
        $crate::hit_dyn(&format!($($arg)*))
    };
}

/// Disabled: expands to nothing (the arguments are not evaluated).
#[cfg(not(feature = "enabled"))]
#[macro_export]
macro_rules! hitf {
    ($($arg:tt)*) => {};
}

#[cfg(feature = "enabled")]
mod imp {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, Once};

    type Sites = Vec<(&'static str, &'static AtomicU64)>;
    static SITES: Mutex<Sites> = Mutex::new(Vec::new());
    static DYN: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
    static THREAD: Once = Once::new();

    /// Registers a call-site counter (called once per site by the macro).
    pub fn register(key: &'static str, c: &'static AtomicU64) {
        if let Ok(mut s) = SITES.lock() {
            s.push((key, c));
        }
        THREAD.call_once(|| {
            let _ = std::thread::Builder::new()
                .name("portcov-dump".into())
                .spawn(|| loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    dump();
                });
        });
    }

    /// Counts by a runtime name (e.g. `state:PsRangedAim`).
    pub fn hit_dyn(key: &str) {
        THREAD.call_once(|| {
            let _ = std::thread::Builder::new()
                .name("portcov-dump".into())
                .spawn(|| loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    dump();
                });
        });
        if let Ok(mut d) = DYN.lock() {
            if let Some(v) = d.get_mut(key) {
                *v += 1;
            } else {
                d.insert(key.to_string(), 1);
            }
        }
    }

    /// Writes all counters to `$GOTHIC_PORTCOV_OUT` (atomically via a temp file).
    pub fn dump() {
        let path = std::env::var("GOTHIC_PORTCOV_OUT")
            .unwrap_or_else(|_| ".local/portcov/out.json".to_string());
        let mut all: BTreeMap<String, u64> = BTreeMap::new();
        if let Ok(s) = SITES.lock() {
            for (k, c) in s.iter() {
                *all.entry((*k).replace(char::from(92u8), "/")).or_insert(0) += c.load(Ordering::Relaxed);
            }
        }
        if let Ok(d) = DYN.lock() {
            for (k, v) in d.iter() {
                *all.entry(k.clone()).or_insert(0) += v;
            }
        }
        let mut out = String::from("{\n");
        let n = all.len();
        for (i, (k, v)) in all.iter().enumerate() {
            out.push_str(&format!(
                "  \"{}\": {}{}\n",
                k.replace('"', "'"),
                v,
                if i + 1 < n { "," } else { "" }
            ));
        }
        out.push('}');
        if let Some(dir) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = format!("{path}.tmp");
        if std::fs::write(&tmp, out).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[cfg(feature = "enabled")]
pub use imp::{dump, hit_dyn, register};

/// Disabled: nothing.
#[cfg(not(feature = "enabled"))]
#[inline(always)]
pub fn hit_dyn(_key: &str) {}

/// Disabled: nothing.
#[cfg(not(feature = "enabled"))]
#[inline(always)]
pub fn dump() {}
