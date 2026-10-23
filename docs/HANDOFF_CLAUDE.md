# Übergabe an Claude — Gothic Reforged

Stand: 20.10.2026, Europe/Berlin. Gewünschte Übernahme: ab 11:00 Uhr. Diese Datei ist die Übergabe; es wurde keine automatische Claude-Sitzung gestartet oder Nachricht verschickt.

## Auftrag und realistischer Stand

Der Nutzer möchte Gothic 3 unabhängig in Rust/Bevy neu implementieren, performant und modular genug für spätere Verbesserungen. Codex sollte anfangen, die lokale Installation finden und den Stand dokumentieren. Ausgangszustand des Repositorys: ausschließlich Spezifikationen. Jetzt liegt ein implementiertes M0-Fundament vor. **Noch kein spielbarer Gothic-3-Port und kein erreichtes M1.** Der Viewer zeigt ein synthetisches Zellraster, keine importierte Welt. Performance des Originalspiels oder eines vollständigen Ports ist nicht gemessen.

Arbeitsverzeichnis: `C:\Users\PowerTower\dev\gothic-reforged`. Änderungen liegen im gemeinsamen Arbeitsverzeichnis. `CLAUDE.md` verweist hierher. Produktumfang und Qualitätsanforderungen bleiben in `PROJECT_SPEC.md`, `ARCHITECTURE.md` und `QUALITY_AND_MILESTONES.md` maßgeblich.

## Gefundene Originalinstallation

`D:\SteamLibrary\steamapps\common\Gothic 3`

- 20 PAK-Basisarchive, fünf P00-Dateien, drei CPT-Dateien; zusammen 3.480.589.376 Bytes.
- Rust-Scan indexiert alle 25 PAK/P00 mit insgesamt **153.580 Dateieinträgen**. Die drei CPT-Dateien werden explizit als nicht unterstützt gemeldet.
- Deutsch und Englisch als Spracharchive vorhanden. INI: Text/Audio German, Version 1.75. Versionsressourcen der eigentlichen Spielbinärdatei und Engine: `1,75,20108,14`. Daraus allein keine vollständige CP-Verhaltenskompatibilität ableiten.
- `Gothic3.exe` ist nur der Launcher; `Gothic3Final.exe` startet das Spiel. Eigentliche Systeme in Engine.dll, Game.dll, SharedBase.dll, FileSystem.dll usw.
- Originalinstallation wurde ausschließlich gelesen. Kein Patch installiert, keine Originaldateien geändert und keine Originaldateien ins Git aufgenommen.

## Implementiert

| Paket | Zweck | Grenzen |
| --- | --- | --- |
| `gothic-formats` | G3V0-Archivindex über gepuffertes Read/Seek, selektives zlib/Plain-Lesen mit Limits und Pfadprüfung | Kein Overlay/Mount-Resolver, kein CPT, kein Bulk-Import |
| `gothic-formats::genome` | GENOMFLE-v1-Hülle und vollständige begrenzte rohe Stringtabelle | Noch keine Property-Stream-Semantik |
| `gothic-formats::ximg` | Beobachtetes G3IMG-DXT1/3/5-Subset, geborgte Mips, DDS-Rewrap ohne Neukompression | Andere Bildvarianten, Orientierung und Materialsemantik offen |
| `gothic-importer` | Deterministischer Installationsbericht, Archivstatistik, EXE/DLL-SHA256, INI-Belege | Kein vollständiger Content-Fingerprint oder PE-Versionsparser |
| `gothic-inspector` | `scan`, `list`, `extract`, `ximg-dds` | Ausgabe muss bei Extraktion neu sein; kein Überschreiben |
| `gothic-world` | Bevy-Plugin, deterministischer Zellplaner, Budget pro Frame und Hysterese | Synthetische Residency, keine tatsächlichen Asset-Ladevorgänge |
| `gothic-reforged` | Endlicher Headless-Lauf; optionaler 3D-Rasterviewer mit Flugkamera | Keine Originalgeometrie, Physik, NPCs oder Spielmechanik |

Bevy **0.20.0**, Rust **1.98.1**, Cargo.lock vorhanden. CI-Workflow für Windows angelegt; remote CI noch nicht ausgeführt. Grafische Features sind opt-in. Keine Physikbibliothek voreilig ausgewählt.

## Starten auf diesem Rechner

Der Standard-Rustkanal ist MSVC, aber `link.exe` und Windows-SDK-Bibliotheken fehlen. Der bereits installierte GNU-Rustkanal funktioniert für einfache Pakete. Für Bevy fehlte zusätzlich ein vollständiges natives Toolset.

Projektlokal liegt jetzt LLVM-MinGW unter `.local/tools/llvm-mingw-20261006-ucrt-x86_64/`. Download aus dem [offiziellen Release 20261006](https://github.com/mstorsjo/llvm-mingw/releases/tag/20261006). ZIP-SHA256 gegen GitHub-Release-Digest geprüft: `317492c456aa27ee607a5919f1d2d38dcdc1112516a24d0bf4b00d078f52d17a`. Metadaten: `.local/tools/llvm-mingw-release.json`. Keine globale Toolchain-/PATH-Konfiguration geändert.

Der Wrapper setzt PATH nur für seinen Aufruf und `RUSTFLAGS=-C link-self-contained=yes`, damit Rusts GNU-Runtimebibliotheken mit LLVM-MinGW zusammenpassen:

```powershell
./scripts/dev.ps1 test
./scripts/dev.ps1 clippy
./scripts/dev.ps1 scan > .local/installation.json
./scripts/dev.ps1 headless
./scripts/dev.ps1 viewer
./scripts/dev.ps1 smoke
```

`smoke` soll nach 90 Frames `.local/viewer-smoke.png` aufnehmen und nach 240 Frames schließen. Steuerung: WASD, rechte Maustaste zum Blickdrehen, Space/Ctrl vertikal, Shift schneller.

Für CLI-Aufrufe ohne Bevy genügt auf diesem Rechner beispielsweise:

```powershell
cargo +stable-x86_64-pc-windows-gnu run -p gothic-inspector --locked -- list "D:\SteamLibrary\steamapps\common\Gothic 3\Data\Quests.pak"
cargo +stable-x86_64-pc-windows-gnu run -p gothic-inspector --locked -- ximg-dds "D:\SteamLibrary\steamapps\common\Gothic 3\Data\_compiledImage.pak" "_Intern/blue.ximg" ".local/blue.dds"
```

Auf anderen Rechnern mit vollständiger nativer Rust-Toolchain funktionieren normale Cargo-Befehle. Der lokale Wrapper ist kein allgemeiner plattformübergreifender Installer.

## Prüfstand

Wird vor Abschluss des Codex-Starts um die endgültigen Build-/Laufresultate ergänzt. Bereits bestanden: neun Format- und vier Importer-Tests vor Ergänzung des allgemeinen GENOMFLE-Lesers, vollständiger Rust-Installationsscan mit 25 indexierten Archiven und drei explizit nicht unterstützten CPT-Dateien. Graphischer Erstbuild läuft zum Zeitpunkt dieses Zwischenstands.

## Befunde und lokale Belege

- `research/INSTALLATION.md`: Installations-/Sprach-/Versionsbelege.
- `research/G3V0_FORMAT.md`: unterstützte Archivstruktur und Parserlimits.
- `research/NATIVE_FINDINGS.md`: erfolgreiche REA/Ghidra-Analyse von Launcher und FileSystem.dll, Evidence-IDs, Hashes, Engine-Export-Seeds.
- `research/RESOURCE_FORMAT_SEEDS.md`: XIMG-Layout an 26 echten Texturen geprüft, GENOMFLE-Trailer, kleines XCMSH-Beispiel, Quellen und Lizenzhinweise.
- `.local/installation.json`: vollständiger maschinenlesbarer Rust-Bericht.
- `.local/research/launcher-{evidence,snapshot}.json`, `filesystem-{evidence,snapshot}.json`: weiterverwendbare REA-Belege/Snapshots.
- `.local/research/native-pe-inventory.json`: DLL-Exporte mit RVAs und Hashes.
- `.local/research/sample.ximg`, `sample.xcmsh`, `ximg-survey.json`: nur lokale Originalproben und Beobachtungen, ausdrücklich nicht für Git.

Unabhängige Formatprüfung zusätzlich zum Rust-Code: 26 ausgewählte echte XIMG-Dateien in der dokumentierten Stichprobe; zweiter separater Rohdatencheck mit 29 Texturen ohne Tabellen-/Größenabweichung. Ein unabhängig aus der kleinen XIMG-Probe erzeugter DDS-Bytestrom wurde von Pillow dekodiert. Das bestätigt den Containeraufbau; endgültige Rust-Konverter-/Rendererprüfungen siehe Prüfstand.

REA ist installiert und die Ghidra-Analyse kleiner Binaries funktioniert. Eine gezielte Engine.dll-Abfrage scheiterte nach 330 Sekunden am Provider-Startlimit; **keine Engine-Pseudocode-Erkenntnisse aus diesem Versuch**. Fehler unter `.local/research/engine-analysis-status.json`. Alle REA-Ziele wurden geschlossen. Bei Bedarf FileSystem-Snapshot wiederverwenden. Engine-Analyse braucht ein längeres serverseitiges Timeout oder ein bewusst reduziertes Analyseprofil; unveränderte Anfrage nicht blind wiederholen.

## Wichtige offene Details

- Acht Einträge in `_compiledMesh.p00` haben offset/stored/decoded size gleich null. Werden als `payloadless()` erhalten; Lesen ist gesperrt. Lösch-/Overlay-Semantik unbekannt. Bit 0x2000 kommt auch bei normalen Einträgen vor und ist kein belegter Löschmarker. Referenzflag 0x8000 separat vorhanden, aber nicht mit umfassend verifiziertem Mountverhalten verwechseln.
- Kopf-Reserved-Feld ist nicht immer null. Version/revision sind in diesen Archiven 0/0.
- Indexieren prüft keine vollständige Payload-/Asset-Kompatibilität. Compression 0/2 unterstützt, Verschlüsselung und andere Methoden explizit abgewiesen.
- XIMG-Mipdaten liegen klein nach groß; DDS groß nach klein. Mipanzahl aus Datei lesen, nicht aus der längsten Kante erzwingen. Farbraum, Normalmaps, Alpha-Interpretation, Orientierung und Materialzuordnung sind gesonderte Aufgaben.
- Weltkoordinaten, Einheiten, Händigkeit, Mesh-/Index-/Vertexformate, Ardea-Lage, Skeletons und Bewegungen sind nicht implementiert.
- Keine Quests, Dialoge, Inventare, Saves, Kämpfe, Navigation, Streaming-I/O oder Gameplay-Parität.

## Empfohlener nächster konkreter Schritt

1. Vollständige Prüfung über Wrapper wiederholen, lokale Berichte lesen und vorhandene Evidence wiederverwenden.
2. GENOMFLE-Property-Stream mit eigener begrenzter Rust-Implementierung erschließen. Kleines lokales `sample.xcmsh` und dessen sechs Strings sind ein brauchbarer Ausgangspunkt; anschließend ein echtes sichtbares Mesh statt eines Collision-Meshs.
3. Meshvertices/Indizes, Materialreferenzen und XIMG-Texturen zusammenführen. Zuerst ein echtes Originalobjekt in einem klar benannten Bevy-Assetviewer rendern; Koordinaten und Orientierung prüfen.
4. Erst dann originale Ardea-Sektoren/Platzierungen, Terrain und Kollision importieren. Benötigte Payloads asynchron und budgetiert laden; Aufenthaltszellen nicht mit tatsächlich geladenen Assets verwechseln.
5. Parallel Daten-/ID-/Questverträge aus den vorhandenen Spezifikationen konkretisieren. Keine Mechanik durch frei erfundene Ersatzdaten als fertig erklären.

M1 gilt erst mit echter importierter Ardea-Geometrie, Held/Animation/Kollision, belegter Interaktion und Save/Reload als erreicht. Ziel ist eine vollständige, veränderbare Implementierung; dieser Start ist bewusst eine belastbare Import- und Enginegrundlage.

## Stand 20.10.2026 nachmittags (Claude-Sitzung)

Build: Git Bash `source .local/env.sh && cargo +stable-x86_64-pc-windows-gnu <cmd> --locked` (dev.ps1 bricht bei cargo-stderr ab). Branch `dev/engine-continuation`, lokal, nicht gepusht.

Fertig (geprüft): Materialien/Texturen (xshmat), VFS + Weltindex, Spieldaten (Strings/Quests/Infos/Templates/Terrain), Property-Streams (Items/NPCs typisiert), Actors/Motions (Held animiert), begehbares Ardea (`--walk`, ~4 ms/Frame), crates/gameplay (Quest-/Dialog-Engine, questlint 559/570), crates/ui (Dialog/Journal/Inventar/HUD), crates/audio (Voice 97 %, Musik, SFX). Regeln: docs/research/GAME_RULES.md, COMBAT_AI_RULES.md.

Bei Sitzungsende evtl. unfertig (Arbeitsbaum prüfen!): crates/animation (Blending/Locomotion/Crowd), Terrain-Blending/Vegetation/Wasser (walk.rs/world_loader.rs), crates/sim (Kampf/KI).

Nächster Schritt: Integration — Held+Animation in Ardea, NPCs aus npcs.json mit Routinen, Ansprechen (E) → crates/ui Dialog + crates/audio Voice, Quest-Log; danach Kampf aus crates/sim. Screenshots unter .local/milestones/.

## VR (Stand 23.10.2026)

Übergabe für die Sitzung auf dem Spiele-PC: `docs/vr/PC_HANDOFF.md` (was fertig ist, was zum Testen im Headset und im Spiel fehlt, was die Installation braucht).
