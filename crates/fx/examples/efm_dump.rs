use gothic_fx::efm::EffectMap;
fn main() {
    let path = std::env::args().nth(1).expect("g3.efm path");
    let map = EffectMap::parse(&std::fs::read(path).unwrap()).unwrap();
    println!("{} effects", map.len());
    for n in std::env::args().skip(2) {
        println!("{n}: {:#?}", map.lookup(&n));
    }
}
