fn main() {
    // meshopt (via gothic-cook) is C++: link the shared libc++ shipped in the APK.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=c++_shared");
    }
}
