fn u32at(b:&[u8],o:usize)->u32{u32::from_le_bytes(b[o..o+4].try_into().unwrap())}
fn f32at(b:&[u8],o:usize)->f32{f32::from_le_bytes(b[o..o+4].try_into().unwrap())}
fn main(){
    let b=std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let pat=b"NXS\x01MESH";
    let pos:Vec<usize>=(0..b.len()-8).filter(|&i|&b[i..i+8]==pat).collect();
    println!("{pos:?} len {}",b.len());
    for p in pos{
        let q=p+8;
        let nv=u32at(&b,q+20) as usize; let nt=u32at(&b,q+24) as usize;
        println!("@{p} ver={} flags={:x} {} nv={nv} nt={nt}",u32at(&b,q+4),u32at(&b,q+12),u32at(&b,q+16));
        let vs=q+28; let ve=vs+nv*12;
        println!(" v0=({},{},{})",f32at(&b,vs),f32at(&b,vs+4),f32at(&b,vs+8));
        let w=if nv<=256{1}else{2};
        let mut mx=0u32;
        for i in 0..nt*3{ let v=if w==1{b[ve+i] as u32}else{u16::from_le_bytes([b[ve+i*2],b[ve+i*2+1]]) as u32}; mx=mx.max(v);}
        let ie=ve+nt*3*w;
        println!(" w={w} maxidx={mx} next={:02x?}",&b[ie..(ie+64).min(b.len())]);
    }
}
