//! Validation for the finite controlled-wind screenshot run.
//!
//! This parser is deliberately independent of Bevy so its exact source can be tested in the
//! lightweight harness without linking the game binary.

/// Parse the opt-in wind-time list and validate the mode combinations.
///
/// `shots_count` is the count after parsing the existing `--shots` syntax.
pub(super) fn parse_wind_shot_times(
    args: &[String],
    shots_count: usize,
    smoke: bool,
    nowind: bool,
    boundary_probe: bool,
) -> Result<Option<Vec<f64>>, String> {
    let occurrences: Vec<usize> = args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| (arg == "--wind-shot-times").then_some(index))
        .collect();
    if occurrences.is_empty() {
        return Ok(None);
    }
    if occurrences.len() != 1 {
        return Err("--wind-shot-times may be specified only once".into());
    }
    if !smoke {
        return Err("--wind-shot-times requires --smoke".into());
    }
    if args.iter().any(|arg| arg == "--qa") {
        return Err("--wind-shot-times cannot be combined with --qa".into());
    }
    if args.iter().any(|arg| arg.starts_with("--bench")) {
        return Err("--wind-shot-times cannot be combined with benchmark options".into());
    }
    if nowind {
        return Err("--wind-shot-times cannot be combined with GR_NOWIND".into());
    }
    if boundary_probe {
        return Err("--wind-shot-times cannot be combined with GR_WIND_CLOCK_PROBE_START".into());
    }

    let index = occurrences[0];
    let value = args
        .get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| "--wind-shot-times requires comma-separated seconds".to_string())?;
    let times = value
        .split(',')
        .map(|part| {
            let seconds = part.trim().parse::<f64>().map_err(|_| {
                format!("invalid --wind-shot-times value {part:?}; expected finite seconds")
            })?;
            if !seconds.is_finite() || !(0.0..=86_401.0).contains(&seconds) {
                return Err(format!(
                    "--wind-shot-times values must be finite seconds from 0 through 86401 (got {part:?})"
                ));
            }
            Ok(seconds)
        })
        .collect::<Result<Vec<_>, String>>()?;
    if times.is_empty() || times.len() > 16 {
        return Err("--wind-shot-times requires 1 through 16 sample times".into());
    }
    if shots_count == 0 {
        return Err("--wind-shot-times requires a nonempty --shots list".into());
    }
    if times.len() != shots_count {
        return Err(format!(
            "--wind-shot-times has {} times but --shots has {shots_count} viewpoints",
            times.len()
        ));
    }
    if !times.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err("--wind-shot-times values must be strictly increasing".into());
    }
    Ok(Some(times))
}

/// Prior synthetic render sample paired with a requested 60 Hz wind sample.
pub(super) fn previous_sample_time(requested_seconds: f64) -> f64 {
    (requested_seconds - 1.0 / 60.0).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn parse(values: &[&str], shot_count: usize) -> Result<Option<Vec<f64>>, String> {
        parse_wind_shot_times(&args(values), shot_count, true, false, false)
    }

    #[test]
    fn mode_is_off_when_flag_is_absent() {
        assert_eq!(parse(&["--smoke", "--shots", "a:0,0,0,0,0"], 1), Ok(None));
    }

    #[test]
    fn accepts_bounded_strictly_increasing_times_matching_views() {
        assert_eq!(
            parse(
                &[
                    "--smoke",
                    "--shots",
                    "a:0,0,0,0,0;b:1,1,1,0,0;c:2,2,2,0,0",
                    "--wind-shot-times",
                    "0, 3599.0, 86401",
                ],
                3,
            ),
            Ok(Some(vec![0.0, 3599.0, 86_401.0]))
        );
    }

    #[test]
    fn rejects_mode_combinations_and_malformed_lists() {
        let flag = "--wind-shot-times";
        assert!(parse_wind_shot_times(&args(&[flag, "0"]), 1, false, false, false).is_err());
        assert!(
            parse_wind_shot_times(&args(&["--smoke", flag, "0"]), 1, true, true, false).is_err()
        );
        assert!(
            parse_wind_shot_times(&args(&["--smoke", flag, "0"]), 1, true, false, true).is_err()
        );
        for value in ["", "NaN", "inf", "-0.1", "86401.1", "1,1", "2,1"] {
            assert!(parse(&["--smoke", flag, value], 2).is_err(), "{value:?}");
        }
        assert!(parse(&["--smoke", flag, "0"], 0).is_err());
        assert!(parse(&["--smoke", flag, "0,1"], 1).is_err());
        assert!(parse(&["--smoke", "--bench-camera", flag, "0"], 1).is_err());
        assert!(parse(&["--smoke", "--qa", flag, "0"], 1).is_err());
        assert!(parse(&["--smoke", flag, "0", flag, "1"], 1).is_err());
    }

    #[test]
    fn rejects_more_than_sixteen_samples() {
        let times = (0..17)
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse(&["--smoke", "--wind-shot-times", &times], 17).is_err());
    }

    #[test]
    fn previous_sample_is_one_sixtieth_before_target_and_never_negative() {
        assert_eq!(previous_sample_time(0.0), 0.0);
        assert_eq!(previous_sample_time(1.0 / 120.0), 0.0);
        assert!((previous_sample_time(3_600.0) - (3_600.0 - 1.0 / 60.0)).abs() < 1e-12);
        assert!((previous_sample_time(86_400.0) - (86_400.0 - 1.0 / 60.0)).abs() < 1e-12);
    }
}
