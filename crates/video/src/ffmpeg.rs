//! Locating the ffmpeg / ffprobe executables and asking what they can decode.
//!
//! The binaries come from `GOTHIC_FFMPEG` / `GOTHIC_FFPROBE` if set, else `ffmpeg` / `ffprobe`
//! next to the game executable, else from `PATH`.
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

fn locate(env: &str, name: &str) -> PathBuf {
    if let Some(p) = std::env::var_os(env) {
        return PathBuf::from(p);
    }
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    if let Ok(dir) = std::env::current_exe()
        && let Some(dir) = dir.parent()
        && dir.join(&exe).is_file()
    {
        return dir.join(exe);
    }
    PathBuf::from(name)
}

pub fn ffmpeg() -> PathBuf {
    locate("GOTHIC_FFMPEG", "ffmpeg")
}

pub fn ffprobe() -> PathBuf {
    locate("GOTHIC_FFPROBE", "ffprobe")
}

/// A command with no window on Windows and quiet logging.
pub(crate) fn command(program: PathBuf) -> Command {
    let mut c = Command::new(program);
    c.arg("-hide_banner").stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Both executables run.
pub fn available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        let ok = |p: PathBuf| {
            command(p)
                .arg("-version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        ok(ffmpeg()) && ok(ffprobe())
    })
}

/// Names of the decoders ffmpeg has (`binkvideo`, `h264`, ...); empty without ffmpeg.
pub fn decoders() -> &'static [String] {
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    LIST.get_or_init(|| {
        let Ok(out) = command(ffmpeg())
            .arg("-decoders")
            .stderr(Stdio::null())
            .output()
        else {
            return Vec::new();
        };
        parse_decoders(&String::from_utf8_lossy(&out.stdout))
    })
}

/// `ffmpeg -decoders` output: lines like ` V....D binkvideo   Bink video` after a `------`.
pub(crate) fn parse_decoders(text: &str) -> Vec<String> {
    text.lines()
        .skip_while(|l| !l.trim_start().starts_with("------"))
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let flags = it.next()?;
            (flags.len() == 6).then(|| it.next().map(str::to_string))?
        })
        .collect()
}

pub fn has_decoder(name: &str) -> bool {
    decoders().iter().any(|d| d == name)
}

/// The ffmpeg build can decode Bink video (the cutscene format of the original game).
pub fn supports_bink() -> bool {
    has_decoder("binkvideo")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decoder_list() {
        let text = "Decoders:\n V..... = Video\n ------\n V....D binkvideo            Bink video\n A....D binkaudio_dct        Bink Audio (DCT)\n VFS..D h264                 H.264\n";
        assert_eq!(parse_decoders(text), ["binkvideo", "binkaudio_dct", "h264"]);
    }
}
