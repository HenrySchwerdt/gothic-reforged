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

This mode has not yet been run in a game build. A `ScreenshotCaptured` event confirms that Bevy
produced the requested image; it does not verify a GPU clock-buffer readback or prove visible leaf
displacement. Compare the resulting images at matching camera poses and inspect the foliage region
before drawing conclusions about rendered motion.
