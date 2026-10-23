# VR: Übergabe an die Sitzung auf dem Spiele-PC

Stand: 23.10.2026. Alles Folgende ist auf `dev/engine-continuation` gemergt (PRs #18, #20, #46–#48,
#71–#74). Entwickelt und getestet wurde **nur mit synthetischen Inhalten** (Graubox, Kapsel-Dummys,
prozedurale Waffen) und **ohne echtes Headset**. Nichts davon belegt Spielbarkeit mit Gothic-3-Daten.

## Was fertig und getestet ist

| Bereich | Crate | Stand |
| --- | --- | --- |
| Eingabe, Rig, Fortbewegung, Körper-IK | `gothic-vr` | headless getestet; Desktop-Emulator (Maus = Hand) |
| OpenXR | `gothic-vr` (Feature `openxr`) | **nur Eingabe + Vibration** (headless Session), gegen Monado getestet; **kein Bild ins Headset** |
| Physik-Hände, Greifen, Werfen, Gewicht/Stärke | `gothic-vr-physics` | ruhiges Schwert in der Hand, Zweihand: zweite Hand zielt |
| Nahkampf | `gothic-vr-melee` | Klingen stoppen an Körpern, gerade Stiche stecken, Schwert gegen Schwert mit Rückstoß |
| NPC (aktives Ragdoll, Kampf-KI) | `gothic-vr-npc` | headless Duell-Tests |
| Bogen | `gothic-vr-archery` | Einnocken, Zielen Hand→Auflage, Ballistik |
| HUD, Interaktion | `gothic-vr-ui`, `gothic-vr-interact` | mit Fake-Daten / Stub-Core |
| Brücke zum Spiel | `gothic-vr-bridge` | Level-Dreiecke → avian, Runtime-Akteure → Proxys mit Trefferzonen, Treffer → `Command::Attack` |
| Sandbox | `apps/vr-sandbox` | Desktop-Modus; `--showcase slash\|stab\|block\|bow --screenshot x.png` |

Details: `docs/vr/README.md`, `docs/vr/PHYSICS.md`, `docs/vr/OPENXR.md`.

## Was fehlt (in dieser Reihenfolge sinnvoll)

1. **Bild ins Headset** (`docs/vr/OPENXR.md`, „Rendering into the headset"): OpenXR-Session mit
   `XR_KHR_vulkan_enable2` auf wgpu-hals Vulkan-Instanz/-Gerät (Bevy `RenderCreation::Manual`),
   Swapchain-Bilder als wgpu-Texturen für die zwei `XrEyeCamera`s, `xrWaitFrame` / `xrBeginFrame` /
   `xrEndFrame` um Bevys Render. Zuerst in `apps/vr-sandbox` (synthetisch, kein Spielinhalt nötig),
   dann im Spiel. Erst danach ist überhaupt etwas im Headset testbar.
2. **`--vr` in `apps/game`**: `apps/game` nutzt die VR-Crates noch gar nicht. Einbau:
   `VrPlugin` + `VrPhysicsPlugin` + `VrMeleePlugin` + `VrArcheryPlugin` + `VrBridgePlugin`
   (Core-Tick aus der Brücke statt `CorePlugin`), VR-Kamera/Hände statt Desktop-Kamera,
   `VrCommands` → Core-Kommandos, Leveldreiecke über `LevelChunkMessage` in avian.
3. **Daten-Abbildung (braucht die Installation):**
   - Skelett: `gothic_vr::body::Joint` / `gothic_vr_melee::Bone` → Gothic-Knochen (Darstellung,
     Trefferzonen, Ragdoll-Proportionen); NPC-Trefferzonen aus dem echten Skelett statt
     `standing_zones`.
   - Waffen: `props::*` / `profiles::profile_for` durch Item-Vorlagen ersetzen (Masse, Länge, Griff,
     Klinge/Spitze); ausgerüstete Waffe in die Hand.
   - Schaden: `HitMessage` / `ArrowHitMessage` in die originale Schadensformel (über die Brücke ist
     das `Command::Attack`, also die Kampfregeln des Cores).
   - Stärke: `VrStrength` aus dem `STR` des Helden.
4. **Am Headset abstimmen:** Vibrationsstärken, Rückstoß (`STAGGER_STRENGTH`), Wundwiderstand
   (`WOUND_DRAG`), Hand-PD, Komfort (Vignette, Snap-Turn).

## Offene Punkte ohne VR-Bezug

- `check`-Job: 42 Clippy-Fehler in `apps/game` (Nutzer behebt sie); restliche Base-Lints in PR #75.
- Orks wirken zusammengekauert: `cargo run --release -p gothic-animation --example shape_probe`
  auf der Installation laufen lassen; die Ausgabe zeigt, ob Clip, Bind-Pose oder Skalierung schuld
  ist (PR #72). Danach den eigentlichen Fix bauen.

## Regeln (aus `CLAUDE.md`)

Originalinstallation nur lesen; Berichte, Proben, Screenshots nur unter `.local/`. Keine
Originalinhalte oder dekompilierten Körper committen. Getestete Unterstützung von Plänen trennen;
aus einem erfolgreichen Build nie Spielbarkeit ableiten.
