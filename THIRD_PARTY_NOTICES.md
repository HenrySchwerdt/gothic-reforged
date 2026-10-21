# Research attribution

The G3V0 reader was informed by the file-layout documentation expressed in [G3Archive](https://github.com/gekonnn/G3Archive), consulted 2026-10-20, based on NicoDE's G3Pak specification. The implementation in this project is Rust. Preserve this notice for the consulted MIT-licensed work:

MIT License

Copyright (c) 2024 gekoN

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

# Souls-combat reference

`crates/combat-souls` (the opt-in Souls-style combat mode) was adapted from the structure of [ELDEN-RING-Playermodel-Rust](https://github.com/Funny-Bones/ELDEN-RING-Playermodel-Rust) at commit `177a3b5ec14a9bde39b9ab458a3400b4038224e3` (consulted 2026-10-21): the engine-free 60 Hz player state machine with frame-based actions, per-input cancel windows, one-slot input buffer, roll-on-release / hold-to-sprint, guard-counter window and hit-stop. The Rust code, action table, tuning values and enemy model in this repository are our own; no Elden Ring game data, animations or extracted numbers are used. Files derived from the reference (`crates/combat-souls/src/hero.rs`) keep a pointer to this notice. Preserve this notice for the MIT-licensed work:

MIT License

Copyright (c) 2026 Funny-Bones and contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

Rust dependencies retain their respective licenses. No original Gothic 3 data is included in this repository. Local analysis output and extracted samples belong under ignored `.local/`.
