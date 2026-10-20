//! Local build adapter: GNU dlltool asks for GNU-as flags; LLVM assembles the data.
//! Used only on this workstation without a full GNU binutils installation.
use std::{
    env,
    path::PathBuf,
    process::{Command, ExitCode},
};
fn main() -> ExitCode {
    let Some(parent) = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    else {
        return ExitCode::FAILURE;
    };
    let clang = parent.join("../llvm-mingw-20261006-ucrt-x86_64/bin/clang-23.exe");
    let args: Vec<_> = env::args_os().skip(1).filter(|s| s != "--64").collect();
    match Command::new(clang)
        .args(["--target=x86_64-w64-windows-gnu", "-c"])
        .args(args)
        .status()
    {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(error) => {
            eprintln!("GNU-as adapter: {error}");
            ExitCode::FAILURE
        }
    }
}
