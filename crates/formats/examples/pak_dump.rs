//! Debug: list/dump members of a pak. Usage: pak_dump <pak> list [substr] | get <path> <out>
use gothic_formats::PakArchive;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut p = PakArchive::open(&a[1]).expect("open");
    if a[2] == "list" {
        let f = a.get(3).cloned().unwrap_or_default();
        for e in p.entries.iter().filter(|e| e.path.contains(&f)).take(40) {
            println!("{} {}", e.path, e.size);
        }
    } else if a[2] == "sample" {
        let step: usize = a[4].parse().unwrap();
        for i in (0..p.entries.len()).step_by(step) {
            let n = p.entries[i].path.clone();
            std::fs::write(format!("{}/{}", a[3], n.replace(['{','}'], "")), p.read_payload(i).unwrap()).unwrap();
        }
    } else {
        let i = p.entries.iter().position(|e| e.path == a[3]).expect("nf");
        std::fs::write(&a[4], p.read_payload(i).unwrap()).unwrap();
    }
}
