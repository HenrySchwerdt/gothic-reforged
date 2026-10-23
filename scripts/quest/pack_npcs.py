"""Selects ~60 % of the placed world NPCs (deterministic) and writes the intermediate look/placement table that
`apps/quest/examples/pack_world.rs` turns into the pack's NPC manifest (`world_npcs.gnpc`).

Reads only the user's own local exports: `.local/npcs.json` (property-stream export of every world NPC entity,
`docs/research/PROPERTY_STREAMS.md`) and `.local/layer_flags.json` (initial Enabled flags of the sector layers, written
by the PC game's play mode). Output: `.local/quest/world_npcs_in.tsv` (gitignored).

Candidate rule = the PC game's (`apps/game/src/play/npcs.rs::is_candidate/select_roster`): a character is shown when it
wears a Body item or is a whole-body creature (no parts, skeleton resource), is not PC_Hero, its layer root is enabled
and it is not stored disabled inside the layer.

Selection (towns/camps first, deterministic xxh-free hash of document+name+index):
  tier 0 humanoid (Human, Orc) in a town/camp document (`*_city`, clans, monastery, temple)
  tier 1 humanoid elsewhere
  tier 2 other bipeds / monsters in towns
  tier 3 ground creatures
  tier 4 flyers (Seagull, Bloodfly, Gargoyle ...)
The first ceil(FRACTION * candidates) of the tier-then-hash order are kept.

Routine (no AI/sim yet): the original sim runs `ZS_*` scripts we do not evaluate here, so the idle clip is the family's
standing `Ambient` loop and the "routine" action is derived from the role string the original data carries
(`display_name`: Smith, Alchemist, Woodcutter, Slave, ...). Roles without a mapped work state use a deterministic
standing idle variety (Ambient / ArmsCrossed / HandsOnHips / Think / Listen) - all names exist as original motions
(`Hero_Stand_*_<Action>_Loop_*`, see `.local/hero-motions.txt`).
"""
import hashlib, json, math, os, sys

FRACTION = float(os.environ.get("NPC_FRACTION", "0.6"))
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".local")
OUT = os.path.join(ROOT, "quest", "world_npcs_in.tsv")

HUMANOID = {"Human", "Orc"}
BIPED = {"Goblin", "Skeleton", "Zombie", "Ogre", "Troll", "Golem", "IceGolem", "FireGolem", "Stalker"}
FLYER = {"Seagull", "Bloodfly", "Gargoyle"}

# display_name (original role string) -> routine action (original motion `Action` field), Stand pose.
ROLE_ACTION = {
    "Smith": "Forge", "Alchemist": "Alchemy", "Woodcutter": "Lumberjack", "Slave": "PickOre",
    "Tired slave": "PickOre", "Eager slave": "PickOre", "Slave from Silden": "PickOre",
    "Freed slave": "FieldWork", "Farmer": "FieldWork", "Grain thief": "FieldWork",
    "Female slave": "Dance", "Shrine": "BookStand",
}
IDLE_VARIETY = ["Ambient", "ArmsCrossed", "HandsOnHips", "Think", "Listen"]

def h01(*parts):
    d = hashlib.sha256("|".join(str(p) for p in parts).encode()).digest()
    return int.from_bytes(d[:8], "little") / float(1 << 64)

def town(doc):
    d = doc.lower()
    return any(k in d for k in ("_city/", "hammerclan", "fireclan", "monastery", "temple", "/story_"))

def stem(f):
    b = f.replace("\\", "/").rsplit("/", 1)[-1]
    return b.rsplit(".", 1)[0] if "." in b else b

def main():
    d = json.load(open(os.path.join(ROOT, "npcs.json")))
    fl = json.load(open(os.path.join(ROOT, "layer_flags.json")))["docs"]
    cand = []
    skipped_disabled = 0
    for i, e in enumerate(d["entities"]):
        if e["name"].lower() == "pc_hero":
            continue
        vis = e.get("visual") or {}
        parts = vis.get("parts") or []
        has_body = any(it["slot"].lower() == "body" for it in e["inventory"]["equipped"])
        creature = not parts and vis.get("skeleton")
        if not (has_body or creature):
            continue
        doc = e["document"].lower()
        f = fl.get(doc)
        if f is not None and (not f.get("root_enabled", True) or e["name"] in f.get("disabled", [])):
            skipped_disabled += 1
            continue
        cand.append((i, e))
    n_sel = math.ceil(FRACTION * len(cand))

    def tier(e):
        sp = e["species"]
        t = town(e["document"])
        if sp in HUMANOID:
            return 0 if t else 1
        if sp in FLYER:
            return 4
        if sp in BIPED:
            return 2 if t else 3
        return 3
    order = sorted(cand, key=lambda ie: (tier(ie[1]), h01(ie[1]["document"], ie[1]["name"], ie[0])))
    sel = order[:n_sel]
    sel.sort(key=lambda ie: ie[0])  # stable file order
    looks, look_id = {}, {}
    lines_n = []
    tiers = {}
    for i, e in sel:
        vis = e["visual"]
        parts = vis.get("parts") or []
        body = next((p for p in parts if p["slot"] == "Slot_Body"), None)
        if body is not None:
            body_s = (stem(body["file"]), body["switch"])
        elif not parts and vis.get("skeleton"):
            body_s = (stem(vis["skeleton"]), vis.get("skeleton_switch", 0))
        else:
            continue
        extra = [(stem(p["file"]), p["switch"]) for p in parts if p["slot"] != "Slot_Body"]
        att = []
        for it in e["inventory"]["equipped"]:
            v = it.get("visual") or {}
            mesh = v.get("mesh")
            if not mesh:
                continue
            guid = it.get("item_entity")
            slot = next((b for g, b in vis.get("attachments", []) if g == guid and g), None) or "Slot_" + it["slot"]
            att.append((mesh, v.get("mesh_switch", 0), slot))
        key = (body_s, tuple(extra), tuple(att))
        if key not in look_id:
            look_id[key] = len(look_id)
            looks[look_id[key]] = key
        sp = e["species"]
        role = e.get("display_name") or ""
        if sp in ("Human",) and role in ROLE_ACTION:
            action = ROLE_ACTION[role]
        elif sp == "Human" and tier(e) <= 1:
            action = IDLE_VARIETY[int(h01("var", e["name"], i) * len(IDLE_VARIETY))]
        else:
            action = "Ambient"
        gx, gy, gz = e["position"]
        yaw = h01("yaw", e["document"], e["name"], i) * math.tau
        t = tier(e)
        tiers[t] = tiers.get(t, 0) + 1
        lines_n.append(("N", str(-gx), str(gy), str(gz), "%.4f" % yaw, str(look_id[key]), action,
                        sp, e["name"], str(t)))
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        for lid, (b, extra, att) in sorted(looks.items()):
            f.write("\t".join(["L", str(lid), "%s@%d" % b, ",".join("%s@%d" % p for p in extra),
                               ",".join("%s@%d@%s" % a for a in att)]) + "\n")
        for l in lines_n:
            f.write("\t".join(l) + "\n")
    print("candidates", len(cand), "(skipped disabled layers/entities:", skipped_disabled, ") selected", len(lines_n),
          "looks", len(looks), "tiers", dict(sorted(tiers.items())))

main()
