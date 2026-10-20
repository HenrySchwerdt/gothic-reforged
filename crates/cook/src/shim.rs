//! Link shim for the vendored astcenc C++ on the llvm-mingw + Rust-bundled-mingw toolchain.
//!
//! astcenc is compiled by llvm-mingw clang against libc++ (`std::__1`), but the final link uses
//! Rust's self-contained libstdc++ (mixing both runtimes collides on exception symbols). astcenc
//! only needs `std::mutex` / `std::condition_variable` (for its multi-thread task manager, which
//! this crate never uses: it encodes single-threaded contexts, one per rayon worker) plus a
//! `bad_function_call` vtable. Provide those few libc++ symbols on top of Win32 SRW locks.
#![cfg(all(feature = "astc", windows, target_env = "gnu"))]
#![allow(unsafe_code, non_upper_case_globals, non_snake_case)]

use std::ffi::c_void;

unsafe extern "system" {
    fn AcquireSRWLockExclusive(lock: *mut c_void);
    fn ReleaseSRWLockExclusive(lock: *mut c_void);
    fn SleepConditionVariableSRW(cv: *mut c_void, lock: *mut c_void, ms: u32, flags: u32) -> i32;
    fn WakeAllConditionVariable(cv: *mut c_void);
}

/// libc++ `std::unique_lock<std::mutex>`: pointer to the mutex, owns flag.
#[repr(C)]
pub struct UniqueLock {
    mutex: *mut c_void,
    owns: bool,
}

// SAFETY (all below): libc++'s `std::mutex` / `std::condition_variable` on Windows are
// layout-compatible with a zero-initialised SRWLOCK / CONDITION_VARIABLE (one pointer each).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ZNSt3__15mutex4lockEv(this: *mut c_void) {
    unsafe { AcquireSRWLockExclusive(this) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ZNSt3__15mutex6unlockEv(this: *mut c_void) {
    unsafe { ReleaseSRWLockExclusive(this) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ZNSt3__118condition_variable10notify_allEv(this: *mut c_void) {
    unsafe { WakeAllConditionVariable(this) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ZNSt3__118condition_variable4waitERNS_11unique_lockINS_5mutexEEE(
    this: *mut c_void,
    lock: *mut UniqueLock,
) {
    unsafe { SleepConditionVariableSRW(this, (*lock).mutex, u32::MAX, 0) };
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _ZNSt3__117bad_function_callD1Ev(_this: *mut c_void) {}
#[unsafe(no_mangle)]
pub static _ZTVNSt3__117bad_function_callE: [usize; 5] = [0; 5];

/// Reference the shim symbols so their object file is pulled in before astcenc's archive.
pub fn keep_linked() -> usize {
    let f: [usize; 6] = [
        _ZNSt3__15mutex4lockEv as *const () as usize,
        _ZNSt3__15mutex6unlockEv as *const () as usize,
        _ZNSt3__118condition_variable10notify_allEv as *const () as usize,
        _ZNSt3__118condition_variable4waitERNS_11unique_lockINS_5mutexEEE as *const () as usize,
        _ZNSt3__117bad_function_callD1Ev as *const () as usize,
        std::ptr::addr_of!(_ZTVNSt3__117bad_function_callE) as usize,
    ];
    f.iter().fold(0, |a, b| a ^ b)
}
