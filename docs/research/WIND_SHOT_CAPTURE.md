# Controlled wind screenshots

The walk smoke runner has an opt-in way to photograph the same fixed viewpoints with explicit
wind-clock times. It does not change Bevy `Time`, simulation time, or normal gameplay timing.

Example (two views at the same camera pose, sampled at two synthetic times):

```text
gothic-reforged --walk --smoke --shots "leaf-t0:38.83,55.5,28.5,0,0.12;leaf-t1:38.83,55.5,28.5,0,0.12" --wind-shot-times "0,0.5"
```

`--wind-shot-times` requires `--smoke`, a nonempty `--shots` list, and exactly one time for each
view. It accepts 1–16 finite, strictly increasing values from 0 through 86401 seconds. It cannot
be combined with benchmark options, `GR_NOWIND`, or `GR_WIND_CLOCK_PROBE_START`. Invalid options
fail before the world loader starts.

For each view, the smoke runner moves the camera, selects that view's requested synthetic time,
and waits for the existing settled-streaming gate. The clock buffer holds the target current
sample and the sample at `max(0, target - 1/60)` as previous until Bevy reports
`ScreenshotCaptured`. Only then can the next view and wind time be selected. Both startup through
the first selected sample and each shot's settle/capture wait have separate 240-second wall-clock
limits; a timeout exits with an error. Logs include the requested time and encoded current/previous
records at selection and immediately before screenshot capture. The shared grass-focus fields
continue to follow the camera.

Validation ran on artifact `.local/artifacts/gothic-reforged-wind-shots.exe` (SHA-256
`9372EAC6354EA91F7A412733AC3D953B7447DFF31DFDBA3729ADDFFE757D5C9A`, source `2a82eb6`). The
`NaN` CLI check exited 1 before loader startup. A six-shot 1280x720 smoke exited 0 with grounded
360/360; each capture record's current and previous CPU f32 arrays exactly matched its selected
record, and four adjacent samples within the hour/day boundary groups preserved current-to-previous
continuity. Six images and the machine-readable checks are under
`.local/qa/wind-shots/` (`validation-summary.json`). Combat QA also passed: magic 9, bow 15, melee
13 assertions, all exit 0, with saves empty before and after.

Reproduce the boundary capture with the same camera pose for each sample:

```text
gothic-reforged --walk --smoke --screenshot .local/qa/wind-shots/boundary --shots "hour-before:38.83,55.5,28.5,0,0.12;hour-at:38.83,55.5,28.5,0,0.12;hour-after:38.83,55.5,28.5,0,0.12;day-before:38.83,55.5,28.5,0,0.12;day-at:38.83,55.5,28.5,0,0.12;day-after:38.83,55.5,28.5,0,0.12" --wind-shot-times "3599.983333333333,3600,3600.016666666667,86399.98333333333,86400,86400.01666666666"
```

These checks confirm CPU-selected clock records and that Bevy produced the requested screenshots.
They do not verify a GPU clock-buffer readback or prove visible leaf displacement, temporal
continuity in rendered foliage, or a jitter cure.
