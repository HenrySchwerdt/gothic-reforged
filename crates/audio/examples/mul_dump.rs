//! `mul_dump FILE [CLASS]`: prints the objects of a `.mul` / `.ams` Genome document.
use anyhow::Result;
use gothic_formats::propstream::PropValue;
fn main() -> Result<()> {
    let mut a = std::env::args().skip(1);
    let doc = gothic_audio::mul::parse(&std::fs::read(a.next().unwrap())?)?;
    let filter = a.next();
    for o in &doc.objects {
        if filter.as_deref().is_some_and(|f| f != o.class) {
            continue;
        }
        let mut s = format!("{}:", o.class);
        for p in &o.properties {
            let v = match &p.value {
                PropValue::Str(x) => x.clone(),
                PropValue::Enum { ty, value } => format!("{ty}={value}"),
                other => format!("{other:?}"),
            };
            s.push_str(&format!(" {}={}", p.name, v));
        }
        println!("{s}");
    }
    Ok(())
}
