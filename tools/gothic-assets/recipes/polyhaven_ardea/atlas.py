# Rebuilds grass / underbrush tiles of the Gothic foliage atlas from CC0 Poly Haven sprites.
# Only the tiles used by G3_Brush_Plant_Grass_05/06/07 and Forest_Underbrush_02 are replaced;
# herb, flower and fern tiles stay original.
import glob, random
from PIL import Image
random.seed(7)
atlas = Image.open(".local/polyhaven/atlas/s1.png").convert("RGBA")

def crop(im):
    b = im.getchannel("A").point(lambda a: 255 if a >= 128 else 0).getbbox()
    return im.crop(b) if b else im

lib = {k: [crop(Image.open(f).convert("RGBA")) for f in sorted(glob.glob(f".local/polyhaven/sprites/lib/{k}_*.png"))]
       for k in ("gm", "bm", "weed")}

def mean_rgb(im):
    px = [p for p in im.getdata() if p[3] >= 128]
    n = max(len(px), 1)
    return [sum(p[c] for p in px) / n for c in range(3)]

def gain(src, dst):
    luma = lambda c: 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    g = max(0.6, min(1.8, luma(dst) / max(luma(src), 1e-3)))
    return [g * max(0.8, min(1.25, (dst[c] / max(src[c], 1e-3)) / (luma(dst) / max(luma(src), 1e-3)))) for c in range(3)]

def fill(rect, picks, count, hmin, hmax, ground=True):
    x0, y0, x1, y1 = rect
    w, h = x1 - x0, y1 - y0
    orig = atlas.crop(rect)
    tile = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    items = []
    for i in range(count):
        kind = random.choice(picks)
        s = random.choice(lib[kind])
        th = int(h * random.uniform(hmin, hmax)) if ground else int(h * random.uniform(hmin, hmax))
        tw = max(1, int(s.width * th / s.height))
        if tw > w * 0.9:
            tw = int(w * 0.9); th = int(s.height * tw / s.width)
        items.append((random.random(), s.resize((tw, th), Image.LANCZOS)))
    for depth, s in sorted(items, key=lambda t: t[0]):
        x = random.randint(-s.width // 4, w - s.width + s.width // 4)
        y = (h - s.height) if ground else random.randint(0, h - s.height)
        tile.alpha_composite(s, (max(0, min(w - s.width, x)), max(0, y)))
    g = gain(mean_rgb(tile), mean_rgb(orig))
    r, gg, b, a = tile.split()
    tile = Image.merge("RGBA", (r.point(lambda v: min(255, int(v * g[0]))), gg.point(lambda v: min(255, int(v * g[1]))),
                                b.point(lambda v: min(255, int(v * g[2]))), a))
    atlas.paste(tile, (x0, y0))
    print(rect, "gain", [round(v, 2) for v in g])

fill((1536, 1200, 2048, 1536), ["gm", "gm", "bm"], 26, 0.55, 1.0)     # Grass_05: meadow blades
fill((0, 22, 513, 277), ["gm", "bm", "bm"], 18, 0.6, 1.0)             # Grass_06: tall stalks
fill((18, 565, 326, 756), ["gm"], 12, 0.6, 1.0)                        # Grass_07: grass tuft
fill((0, 1024, 512, 1536), ["weed"], 16, 0.2, 0.36, ground=False)       # Underbrush_02 (top view)
atlas.save(".local/polyhaven/atlas/s1_new.png")
side = Image.new("RGB", (2048, 1024), (255, 255, 255))
o = Image.open(".local/polyhaven/atlas/s1.png").convert("RGBA").resize((1024, 1024)); n = atlas.resize((1024, 1024))
bg = Image.new("RGBA", (1024, 1024), (235, 235, 235, 255)); side.paste(Image.alpha_composite(bg, o).convert("RGB"), (0, 0))
side.paste(Image.alpha_composite(bg, n).convert("RGB"), (1024, 0)); side.save(".local/polyhaven/atlas/compare.png")
