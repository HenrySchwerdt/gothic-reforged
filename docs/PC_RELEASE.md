# PC-Release lokal paketieren

`scripts/package-pc.ps1` erstellt ein neues, eigenständiges Paket aus einer bereits gebauten EXE und den Runtime-JSON-Dateien. Das Skript baut nichts und löscht keine Dateien. Der Zielordner muss neu sein; er darf weder im Runtime-Quellbaum liegen noch diesen enthalten.

```powershell
.\scripts\package-pc.ps1 `
  -ExePath 'D:\build\GothicReforged.exe' `
  -OutputDir 'D:\releases\GothicReforged-PC'
```

`SourceRuntimeRoot` ist standardmäßig `C:\Users\PowerTower\dev\gothic-reforged`; bei einem anderen Quellbaum kann der Pfad angegeben werden. `BuildProfile` wird standardmäßig als `release-thin` in `build-info.json` eingetragen. Vor dem Kopieren prüft das Skript `items.json`, `npcs.json` und `objects_world.json` sowie den Git-Stand. Die EXE wird nach dem Kopieren per SHA-256 verglichen.

Das Paket enthält `GothicReforged.exe`, `runtime-root.txt`, `LIESMICH.txt`, `build-info.json` und die vorhandenen JSONs unter `runtime/.local`. `skills.json` und `.local/er/er_clips.grcp` werden mitkopiert, wenn vorhanden. `runtime/.local/saves` wird leer angelegt. Alte Spielstände werden nicht übernommen. Die Laufzeit schreibt neue Paket-Spielstände dort ab.

Das Spiel benötigt weiterhin die installierten Originaldateien unter `D:\SteamLibrary\steamapps\common\Gothic 3\Data`; Original-PAKs werden nicht gebündelt. Der globale Cooked-Cache wird als gemeinsam genutzte, schreibgeschützte Quelle behandelt. Das Paket ist ein Integrationsstand und keine vollständige Gothic-3-Implementierung.

Für einen reproduzierbaren Leistungslauf kann die EXE mit `--play --bench 10 --bench-walk --bench-name pc-release` gestartet werden. Der Benchmark schaltet VSync selbst aus, verwirft die Aufwärmphase und legt die Messung unter `runtime/.local/bench` ab. Vergleiche nur Läufe mit gleicher Auflösung und denselben Optionen; Messwerte aus einem normalen VSync-Lauf sind keine Messung der maximalen Bildrate. Die Start- und Steuerungshinweise sowie der getrennte Save-Ort stehen zusätzlich in der erzeugten `LIESMICH.txt`.
