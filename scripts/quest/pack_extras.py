"""Copy extra cooked assets (actors/meshes + all textures they need) from the local quest cache into the Ardea pack.
Usage: pack_extras.py <pack_dir> actor:<name.xact> mesh:<name.xcmsh> ...   (the user's own local data only)"""
import os, sys, json, struct, shutil
C = os.path.expandvars(r"%LOCALAPPDATA%\GothicReforged\cache\quest")
sys.path.insert(0, os.path.dirname(__file__))

def fnv(): pass

def asset_id_hex(name):
    # xxh3_64 of lower-case basename, max(1)
    import xxhash
    base = name.replace("\\", "/").rsplit("/", 1)[-1].lower()
    v = xxhash.xxh3_64_intdigest(base.encode()) or 1
    return v, f"{v:016x}"

def secs(path):
    b = open(path, "rb").read()
    n = struct.unpack_from("<I", b, 12)[0]
    out = []
    for i in range(n):
        o, l = struct.unpack_from("<QQ", b, 32 + 16 * i)
        out.append(b[o:o + l])
    return out

def mat_table():
    s = secs(os.path.join(C, "materials.gmat"))[0]
    recs = {}
    for i in range(len(s) // 160):
        r = s[i * 160:(i + 1) * 160]
        mid, = struct.unpack_from("<Q", r, 0)
        cnt, = struct.unpack_from("<I", r, 20)
        tex = []
        for l in range(2):
            tex += list(struct.unpack_from("<2Q", r, 32 + 48 * l))
        recs[mid] = [t for t in tex if t]
    return recs

def main():
    pack = sys.argv[1]
    mats = mat_table()
    tex = set()
    for item in sys.argv[2:]:
        kind, name = item.split(":", 1)
        v, h = asset_id_hex(name)
        sub = "actors" if kind == "actor" else "meshes"
        ext = "gact" if kind == "actor" else "gmsh"
        src = os.path.join(C, sub, f"{h}.{ext}")
        if not os.path.isfile(src):
            print("MISSING", item); continue
        os.makedirs(os.path.join(pack, sub), exist_ok=True)
        shutil.copy(src, os.path.join(pack, sub, f"{h}.{ext}"))
        s = secs(src)
        if kind == "actor":
            m = s[2]
            for i in range(len(m) // 48):
                d, n_, sh = struct.unpack_from("<3Q", m, i * 48)
                if d: tex.add(d)
                for t in mats.get(sh, []): tex.add(t)
        else:
            subs = s[3]
            for i in range(len(subs) // 80):
                mid, = struct.unpack_from("<Q", subs, i * 80)
                for t in mats.get(mid, []): tex.add(t)
        print("ok", item, h)
    os.makedirs(os.path.join(pack, "textures"), exist_ok=True)
    n = 0
    for t in tex:
        f = os.path.join(C, "textures", f"{t:016x}.gtex")
        if os.path.isfile(f):
            shutil.copy(f, os.path.join(pack, "textures", f"{t:016x}.gtex")); n += 1
    print("textures copied", n, "of", len(tex))
main()
