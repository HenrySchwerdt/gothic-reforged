# Original Gothic 3 UI (v1.75.14 / CP) - layout data and art

Evidence: `Data/gui.pak` (79 members), `ge3.INI`, and real screenshots of the installed game
(1024x768, German) kept locally in `.local/ref/` (not committed). Nothing here is original content.

## gui.pak contents
| member | meaning |
|---|---|
| `resources/g3.gui.res` | text XML (`<resources><controls/><dialogs/>...`), 62 dialogs / 424 controls, **all layouts in a 1024x768 design space** |
| `themes/g3/compiled/theme_g3.thm` | GENOMFLE theme: names of the generic widget bitmaps (caption, listbox, combobox ...) |
| `themes/g3/compiled/theme_g3_00.dds` | 2048x2048 DXT5 widget atlas: cursors (arrow, resize, hourglass), scroll arrows, gold frames, X |
| `icons/compiled/g3_hud_backgrounds_00..17.dds` | 1024x1024 DXT5: 00-08 full-screen menu backgrounds, **09-17 the in-game view panels** |
| `icons/compiled/g3_hud_icons_00.dds` | 512x512: compass rose + wheel, page icons, tab glyphs, quest-map markers, rank medallions |
| `icons/compiled/g3_hud_it_*_00.dds` | item icon atlases (weapons, armor, misc, alchemy, artefacts, documents, perks, recipe, spells) |
| `g3_hud_maps_*`, `g3_hud_slides_*` | world map pieces, loading slides |

`g3.gui.res` window attributes: `GUI.Window.Placement="l,t,r,b"` (right/bottom, **not** width/height),
`GUI.Window.Font.*` (Face "Times New Roman", Height negative = em px: -14, -16, -18, -24; Weight 400/700),
`GUI.View.Bitmap` = original TGA path of the panel. The compiled atlases have **no index**; the
TGAs were packed into atlases at build time. We recovered the rectangles from the alpha layout
(each view is exactly as big as its background) - see `crates/ui/src/atlas.rs`.
`ge3.INI`: `GUI.DefaultFont=Comic Sans MS` is the configurable *menu* font; the screenshots show
Times New Roman Bold (body) and Bold Italic (headings), which is what the resource file asks for.
`Gothic3.ttf` is only the logo/menu face ("original" option) and is the fallback here.

## Colours / text
Body text `#E0C495` (224,196,149), bold serif 16 px; headings italic bold serif ~22-24 px centred in the
panel header, same colour; selected/hover text white-cream. No text outlines, drop shadow only on game
messages. Frames are the art in the panels (gold bevel + dark blue-grey/teal translucent fills).

## Screens (design space 1024x768, origin top-left)
* **HUD** (always visible, also over menus): compass 100x100 at (16,660) = round brass wheel with
  green gem, star rose turning behind it; quick bar 520x56 at (252,688) with 10 slots of 48 px
  (x = 22+48*i, y = 5), slot number top-left (small), stack count bottom-right; three framed bars
  201x19 at x=807 (health y=688 red, endurance y=708 yellow, mana y=728 blue) with the fill
  rectangle at (+12,+4) 176x12; focus (enemy) bar 201x19 at (411,10); enemy name floats above the NPC
  in red; "game messages" are plain cream text in the rectangle (87,308)-(937,524); no toasts.
* **Page strip** (right, 62x500 at (946,76)): exit button 46x46 at (+6,+48), five page buttons at
  y=144+63*k (inventory chest, character, documents, quests/map, quill), selected one lit.
* **Inventory** (combined): character view (14,16,312x638) = three stacked panels "Erfahrung"
  (Stufe, Erfahrung, Nächste Stufe, Lernpunkte), "Attribute" (Stärke, Jagdgeschick, Altes Wissen,
  Schmiedekunst, Diebeskunst, Alchemie, Lebensenergie x/y, Ausdauer x/y, Mana x/y), "Rüstungsschutz"
  (Klingen, Zerschmettern, Geschossen, Feuer, Eis, Energie); label x=24, value right edge x=290,
  rows 23 px apart. Inventory view (634,16,312x638): italic title = current tab name, five category
  tabs (46x26, glyph icons) at (+30,+46), 5x10 grid of 48 px cells from (+26,+85) with scroll bar on
  the right, "Gold  123" bar at y=+580. Item view (336,16,287x350) appears on selection.
* **Character/skills** view (325,16,621x638): seven headed lists (Kampf, Jagd, Magie, Schmieden /
  Diebstahl, Alchemie, Sonstige) + skill description box.
* **Quest log** (14,16,937x638): title "Alle Missionen" (italic, top centre); world map (38,46,442x392)
  with category markers; tabs Aktuell/Erfolgreich/Gescheitert at (503,44) 3x138x44; quest list
  (508,98,410x176) with a small marker icon per entry; detail box (508,300,410x302), empty text
  "Keine Mission ausgewählt"; reputation list bottom-left (rows 22 px: "Ruf in Nordmar" ... value
  right); day/time widget (330,460): sky strip + "1. Tag, 12:23".
* **Dialog**: no name header. Dialog view (22,534,980x213) dark-teal band at the bottom; options list
  at (+60,+22,858x169), plain text lines, hover highlight. NPC/hero subtitle in the Talk view
  (227,100,570x81) at the top, text -14 px bold.
* **Trade**: vendor view (14,16,303x493) left, player view (691,16,304x493) right, transaction strip
  (14,524,980x213) bottom with exchange buttons and both gold totals.
* **Modal boxes**: confirm box 482x143 (dark red), buttons "Ja"/"Nein" 109x37; tip window 450x379
  ("Tipp", green inner panel, checkbox "Keine Tipps mehr anzeigen", "Ok").
* **Main menu**: full-screen backgrounds 00-08 (page 02 = red banner, 1024x768 at the atlas top; implemented as `Screen::MainMenu`); entries are 252x32 buttons at x=386, y=368/400/432/464/496/532 (bold 19 px, centred; "Spiel speichern" greyed without a game); the confirm box (`Modal::Confirm`, 482x143 at 271,202, buttons 109x37 at +64/+300,+84) and the tip window (`Modal::Tip`, 450x379 at 287,154; art stretched/9-sliced from the 350x279 piece) are `ShowModal`/`ModalAnswer` messages. The original also lists menu entries are plain centred
  bold text on the banner (Neues Spiel, Spiel laden, Spiel speichern, Einstellungen, Mitwirkende,
  Spiel beenden); hovered entry white.

## Item icons - status
Searched: item templates (only `Texture` = TGA file name, e.g. `IT_1H_OrcSlayer_01.tga`), `g3.gui.res` (`<bitmaps>` empty),
`_compiledImage.pak` (no hud/icon entries), DDS headers (no metadata), install files (`g3_hud_it` only occurs in gui.pak
member names). The cell order is not alphabetical by icon name or template name (e.g. `IT_1H_FlameSword_01` is cell 31 of
the weapons atlas, alphabetical rank 11-17); atlas groups follow folder-like order (1H, 2H, axes, bows, ammo, crossbows,
shields, staves) and contain ~12 more cells than there are weapon templates, so extra unreferenced icons are interleaved.
The mapping therefore lives in engine code (`gCInventoryStack::CreateIcons`, Game.dll) or in the build tool, not in data.
`g3_hud_it_*` atlases are regular grids of ~64 px cells, but neither `gui.pak` nor the item
templates (`icon` = TGA file name) carry the cell index (order is not alphabetical). The UI therefore
uses category glyphs from `g3_hud_icons_00` until the compile order is found (likely inside
`Gothic3Final.exe`/`GUI.dll` loader code or reproducible from the original icon directory order).
