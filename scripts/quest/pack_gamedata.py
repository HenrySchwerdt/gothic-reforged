"""Extract the small game-data subset the Quest combat bridge needs (items + one Ardea orc) from the local
gitignored exports `.local/items.json` / `.local/npcs.json` into the Ardea pack. User's own data only."""
import json, os, sys
root = os.path.join(os.path.dirname(__file__), "..", "..", ".local")
pack = sys.argv[1]
items = json.load(open(os.path.join(root, "items.json")))
want = {"arrow", "it_1h_orcslayer_01", "it_bow_hunting", "it_arrow", "it_halberd_orc_01", "it_1h_sword_01", "it_2h_sword_01"}
sub = [i for i in items if i["name"].lower() in want or i["name"].lower().startswith("it_arrow") or i["name"].lower().startswith("it_halberd_orc")]
json.dump(sub, open(os.path.join(pack, "items.json"), "w"))
npcs = json.load(open(os.path.join(root, "npcs.json")))
orc = next(e for e in npcs["entities"] if e["name"] == "Orc_GameStartRaider_Warrior_01")
json.dump({"entities": [orc], "templates": []}, open(os.path.join(pack, "orcs.json"), "w"))
print("items", [i["name"] for i in sub], "orc", orc["name"], orc["level"], orc["level_max"])
print(json.dumps(orc["visual"])[:600])
