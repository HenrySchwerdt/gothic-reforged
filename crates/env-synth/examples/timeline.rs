//! Prints the environment frame every game half hour over one day as JSON lines, cycling the
//! weather every four hours. No window; `cargo run -p gothic-env-synth --example timeline`.
use gothic_env_synth::{Environment, WeatherKind};

fn main() -> anyhow::Result<()> {
    let mut env = Environment::default();
    env.weather.automatic = false;
    env.clock.hours = 0.0;
    env.clock.hours_per_second = 1.0 / 60.0; // one game hour per real minute
    let kinds = WeatherKind::ALL;
    let step = 1.0; // second
    let mut last_half_hour = -1i64;
    while env.clock.hours < 24.0 {
        let k = kinds[(env.clock.hours / 4.0) as usize % kinds.len()];
        if env.weather.target() != k {
            env.force_weather(k);
        }
        env.update(step);
        let half = (env.clock.hours * 2.0) as i64;
        if half != last_half_hour {
            last_half_hour = half;
            println!("{}", serde_json::to_string(&env.frame())?);
        }
    }
    Ok(())
}
