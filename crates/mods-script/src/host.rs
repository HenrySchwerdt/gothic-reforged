//! The script host: one sandboxed Rhai engine, the loaded script mods, budgets and hot reload.
use crate::manifest::{Capability, ScriptManifest};
use crate::rng::Rng;
use crate::vocab::{Issued, Line, ScriptCommand, ScriptEvent, Vec3, WorldView};
use anyhow::{Context, Result, bail};
use rhai::{AST, CallFnOptions, Dynamic, Engine, EvalAltResult, FuncArgs, Map};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Limits that keep a mod from stalling or flooding the game.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptConfig {
    /// World seed; every mod's RNG stream is derived from it and the mod id.
    pub seed: u64,
    /// Rhai operations one handler call may use.
    pub ops_per_call: u64,
    /// Rhai operations one mod may use per tick (all its handlers together).
    pub ops_per_tick: u64,
    /// Commands one mod may issue per tick.
    pub commands_per_tick: usize,
    /// Errors (incl. exceeded budgets) after which a mod is disabled until it is reloaded.
    pub max_strikes: u32,
    /// Lines of `log` / `print` output kept.
    pub max_log: usize,
}

impl Default for ScriptConfig {
    fn default() -> Self {
        Self {
            seed: 0,
            ops_per_call: 100_000,
            ops_per_tick: 250_000,
            commands_per_tick: 64,
            max_strikes: 3,
            max_log: 1000,
        }
    }
}

/// Why a script call failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The source does not compile (on load or reload; a reload keeps the old version).
    Compile,
    /// A runtime error, including a call the mod has no capability for.
    Runtime,
    /// The operation budget ran out.
    Budget,
    /// The mod reached `max_strikes` and is disabled until its next reload.
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScriptError {
    #[serde(rename = "mod")]
    pub mod_id: String,
    /// Handler (`on_quest_changed`) or `load` / `reload`.
    pub during: String,
    pub kind: ErrorKind,
    pub message: String,
}

/// A `log` / `print` line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogLine {
    #[serde(rename = "mod")]
    pub mod_id: String,
    pub text: String,
}

/// State the registered API functions see during one call.
struct Ctx {
    world: WorldView,
    mod_id: String,
    caps: BTreeSet<Capability>,
    rng: Rng,
    commands: Vec<ScriptCommand>,
    commands_left: usize,
    log: Vec<LogLine>,
    max_log: usize,
}

impl Ctx {
    fn need(&self, cap: Capability) -> Result<(), Box<EvalAltResult>> {
        if self.caps.contains(&cap) {
            Ok(())
        } else {
            Err(format!(
                "mod {} has no capability '{}' (declare it in mod.toml [script] capabilities)",
                self.mod_id,
                cap.name()
            )
            .into())
        }
    }

    fn issue(&mut self, cap: Capability, c: ScriptCommand) -> Result<(), Box<EvalAltResult>> {
        self.need(cap)?;
        if self.commands_left == 0 {
            return Err(
                format!("mod {}: command budget for this tick used up", self.mod_id).into(),
            );
        }
        self.commands_left -= 1;
        self.commands.push(c);
        Ok(())
    }

    fn log(&mut self, text: &str) {
        if self.log.len() < self.max_log {
            self.log.push(LogLine {
                mod_id: self.mod_id.clone(),
                text: text.to_string(),
            });
        }
    }
}

type Shared = Arc<Mutex<Ctx>>;

fn lock(c: &Shared) -> MutexGuard<'_, Ctx> {
    c.lock().unwrap_or_else(|p| p.into_inner())
}

struct ScriptMod {
    manifest: ScriptManifest,
    /// Entry file for mods loaded from disk.
    path: Option<PathBuf>,
    source: String,
    ast: AST,
    /// Handler functions the script defines (name, parameter count).
    functions: BTreeSet<(String, usize)>,
    /// Persistent mod state, `this` in every handler (survives hot reloads).
    state: Dynamic,
    rng: Rng,
    strikes: u32,
    disabled: bool,
    ops_left: u64,
    commands_left: usize,
}

impl ScriptMod {
    fn has(&self, name: &str, params: usize) -> bool {
        self.functions.contains(&(name.to_string(), params))
    }
}

/// Runs script mods against core events. Call [`ScriptHost::handle`] once per core tick with
/// that tick's events and feed the returned commands to the core.
pub struct ScriptHost {
    engine: Engine,
    ctx: Shared,
    limit: Arc<AtomicU64>,
    used: Arc<AtomicU64>,
    mods: Vec<ScriptMod>,
    config: ScriptConfig,
    errors: Vec<ScriptError>,
    /// Commands issued by `on_load` / `on_reload`, delivered with the next tick.
    pending: Vec<Issued>,
}

impl ScriptHost {
    pub fn new(config: ScriptConfig) -> Self {
        let ctx: Shared = Arc::new(Mutex::new(Ctx {
            world: WorldView::default(),
            mod_id: String::new(),
            caps: BTreeSet::new(),
            rng: Rng::new(config.seed),
            commands: Vec::new(),
            commands_left: 0,
            log: Vec::new(),
            max_log: config.max_log,
        }));
        let limit = Arc::new(AtomicU64::new(config.ops_per_call));
        let used = Arc::new(AtomicU64::new(0));
        let engine = sandboxed_engine(&ctx, &limit, &used);
        Self {
            engine,
            ctx,
            limit,
            used,
            mods: Vec::new(),
            config,
            errors: Vec::new(),
            pending: Vec::new(),
        }
    }

    pub fn config(&self) -> &ScriptConfig {
        &self.config
    }

    /// The world snapshot scripts read (update it from the core between ticks).
    pub fn update_world(&mut self, f: impl FnOnce(&mut WorldView)) {
        f(&mut lock(&self.ctx).world);
    }

    pub fn set_world(&mut self, world: WorldView) {
        lock(&self.ctx).world = world;
    }

    /// Ids of the loaded mods, in dispatch order.
    pub fn mods(&self) -> Vec<&str> {
        self.mods.iter().map(|m| m.manifest.id()).collect()
    }

    pub fn is_disabled(&self, id: &str) -> bool {
        self.mods
            .iter()
            .any(|m| m.manifest.id() == id && m.disabled)
    }

    /// Persistent state (`this`) of a mod.
    pub fn state(&self, id: &str) -> Option<&Dynamic> {
        self.mods
            .iter()
            .find(|m| m.manifest.id() == id)
            .map(|m| &m.state)
    }

    pub fn errors(&self) -> &[ScriptError] {
        &self.errors
    }

    pub fn take_errors(&mut self) -> Vec<ScriptError> {
        std::mem::take(&mut self.errors)
    }

    pub fn take_log(&mut self) -> Vec<LogLine> {
        std::mem::take(&mut lock(&self.ctx).log)
    }

    /// Add a mod from source (tests, tools). Mods dispatch in the order they were added.
    pub fn add_source(&mut self, manifest: ScriptManifest, source: &str) -> Result<()> {
        self.add(manifest, None, source.to_string())
    }

    /// Load one mod folder (`mod.toml` with a `[script]` section).
    pub fn load_dir(&mut self, dir: &Path) -> Result<()> {
        let text = std::fs::read_to_string(dir.join("mod.toml"))
            .with_context(|| format!("reading {}", dir.join("mod.toml").display()))?;
        let manifest =
            ScriptManifest::parse(&text).with_context(|| format!("mod at {}", dir.display()))?;
        let Some(script) = &manifest.script else {
            bail!("mod {} has no [script] section", manifest.id());
        };
        let path = dir.join(&script.entry);
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        self.add(manifest, Some(path), source)
    }

    /// Load every folder under `mods_dir` whose `mod.toml` has a `[script]` section, sorted by
    /// mod id. Returns the ids loaded; mods that fail to load are reported as errors.
    pub fn discover(&mut self, mods_dir: &Path) -> Result<Vec<String>> {
        let mut found = Vec::new();
        for e in std::fs::read_dir(mods_dir)
            .with_context(|| format!("reading {}", mods_dir.display()))?
        {
            let dir = e?.path();
            let Ok(text) = std::fs::read_to_string(dir.join("mod.toml")) else {
                continue;
            };
            match ScriptManifest::parse(&text) {
                Ok(m) if m.script.is_some() => found.push((m.info.id.clone(), dir)),
                Ok(_) => {}
                Err(e) => self.errors.push(ScriptError {
                    mod_id: dir.display().to_string(),
                    during: "load".into(),
                    kind: ErrorKind::Compile,
                    message: format!("{e:#}"),
                }),
            }
        }
        found.sort();
        let mut loaded = Vec::new();
        for (id, dir) in found {
            match self.load_dir(&dir) {
                Ok(()) => loaded.push(id),
                Err(e) => self.errors.push(ScriptError {
                    mod_id: id,
                    during: "load".into(),
                    kind: ErrorKind::Compile,
                    message: format!("{e:#}"),
                }),
            }
        }
        Ok(loaded)
    }

    fn add(
        &mut self,
        manifest: ScriptManifest,
        path: Option<PathBuf>,
        source: String,
    ) -> Result<()> {
        let id = manifest.id().to_string();
        if self.mods.iter().any(|m| m.manifest.id() == id) {
            bail!("script mod {id} is already loaded");
        }
        let ast = self
            .engine
            .compile(&source)
            .map_err(|e| anyhow::anyhow!("mod {id}: {e}"))?;
        let functions = functions(&ast);
        self.mods.push(ScriptMod {
            rng: Rng::for_mod(self.config.seed, &id),
            manifest,
            path,
            source,
            ast,
            functions,
            state: Map::new().into(),
            strikes: 0,
            disabled: false,
            ops_left: self.config.ops_per_tick,
            commands_left: self.config.commands_per_tick,
        });
        let i = self.mods.len() - 1;
        let mut out = Vec::new();
        self.call(i, "on_load", (), &mut out);
        // Commands issued while loading go out with the next tick.
        self.pending.extend(out);
        Ok(())
    }

    /// Dispatch one tick's events to every enabled mod (mod order, then event order) and
    /// return the commands they issued. Budgets are per tick.
    pub fn handle(&mut self, events: &[ScriptEvent]) -> Vec<Issued> {
        for m in &mut self.mods {
            m.ops_left = self.config.ops_per_tick;
            m.commands_left = self.config.commands_per_tick;
        }
        let mut out = std::mem::take(&mut self.pending);
        for i in 0..self.mods.len() {
            for ev in events {
                let name = ev.handler();
                if !self.mods[i].has(name, 1) {
                    continue;
                }
                let arg = ev.to_dynamic(&lock(&self.ctx).world);
                self.call(i, name, (arg,), &mut out);
            }
        }
        out
    }

    /// The subtitle hook: every enabled mod with the `subtitles` capability and a
    /// `fn rewrite_line(line)` may replace the text (returning a string) or leave it (returning
    /// nothing); mods chain in order. `None` if no mod changed it.
    pub fn rewrite_line(&mut self, line: &Line) -> Option<String> {
        let mut text = line.text.clone();
        let mut changed = false;
        let mut sink = Vec::new();
        for i in 0..self.mods.len() {
            let m = &self.mods[i];
            if !m.has("rewrite_line", 1)
                || !m.manifest.capabilities().contains(&Capability::Subtitles)
            {
                continue;
            }
            let current = Line {
                text: text.clone(),
                ..line.clone()
            };
            let arg = current.to_dynamic(&lock(&self.ctx).world);
            if let Some(r) = self.call(i, "rewrite_line", (arg,), &mut sink)
                && let Ok(s) = r.into_string()
            {
                changed |= s != text;
                text = s;
            }
        }
        // A rewrite hook cannot issue commands; they are dropped.
        changed.then_some(text)
    }

    /// Re-read the scripts of mods loaded from disk; a changed one is recompiled (its state
    /// kept, `on_reload` called, a disabled mod enabled again). Returns the ids reloaded. A
    /// script that no longer compiles is reported and keeps running its previous version.
    pub fn poll_reload(&mut self) -> Vec<String> {
        let mut reloaded = Vec::new();
        for i in 0..self.mods.len() {
            let Some(path) = self.mods[i].path.clone() else {
                continue;
            };
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            if source == self.mods[i].source {
                continue;
            }
            let id = self.mods[i].manifest.id().to_string();
            self.mods[i].source = source.clone();
            match self.engine.compile(&source) {
                Ok(ast) => {
                    let m = &mut self.mods[i];
                    m.functions = functions(&ast);
                    m.ast = ast;
                    m.disabled = false;
                    m.strikes = 0;
                    let mut out = Vec::new();
                    self.call(i, "on_reload", (), &mut out);
                    self.pending.extend(out);
                    reloaded.push(id);
                }
                Err(e) => self.errors.push(ScriptError {
                    mod_id: id,
                    during: "reload".into(),
                    kind: ErrorKind::Compile,
                    message: e.to_string(),
                }),
            }
        }
        reloaded
    }

    /// Call `name` of mod `i` if it exists and the mod is enabled, under its budgets.
    fn call(
        &mut self,
        i: usize,
        name: &str,
        args: impl FuncArgs,
        out: &mut Vec<Issued>,
    ) -> Option<Dynamic> {
        let params = args_len(name);
        let m = &mut self.mods[i];
        if m.disabled || !m.has(name, params) {
            return None;
        }
        let id = m.manifest.id().to_string();
        if m.ops_left == 0 {
            let e = ScriptError {
                mod_id: id,
                during: name.into(),
                kind: ErrorKind::Budget,
                message: "operation budget for this tick used up".into(),
            };
            self.strike(i, e);
            return None;
        }
        {
            let mut c = lock(&self.ctx);
            c.mod_id = id.clone();
            c.caps = m.manifest.capabilities();
            c.rng = m.rng;
            c.commands_left = m.commands_left;
            c.commands.clear();
        }
        self.limit
            .store(self.config.ops_per_call.min(m.ops_left), Ordering::Relaxed);
        self.used.store(0, Ordering::Relaxed);
        let mut scope = rhai::Scope::new();
        let options = CallFnOptions::new()
            .eval_ast(false)
            .rewind_scope(true)
            .bind_this_ptr(&mut m.state);
        let r = self
            .engine
            .call_fn_with_options::<Dynamic>(options, &mut scope, &m.ast, name, args);
        let used = self.used.load(Ordering::Relaxed);
        m.ops_left = m.ops_left.saturating_sub(used.max(1));
        {
            let mut c = lock(&self.ctx);
            m.rng = c.rng;
            m.commands_left = c.commands_left;
            out.extend(c.commands.drain(..).map(|c| c.from_mod(&id)));
        }
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                let kind = if matches!(*e, EvalAltResult::ErrorTerminated(..)) {
                    ErrorKind::Budget
                } else {
                    ErrorKind::Runtime
                };
                let message = match kind {
                    ErrorKind::Budget => format!("operation budget exceeded ({used} operations)"),
                    _ => e.to_string(),
                };
                self.strike(
                    i,
                    ScriptError {
                        mod_id: id,
                        during: name.into(),
                        kind,
                        message,
                    },
                );
                None
            }
        }
    }

    fn strike(&mut self, i: usize, e: ScriptError) {
        let m = &mut self.mods[i];
        m.strikes += 1;
        let disable = m.strikes >= self.config.max_strikes && !m.disabled;
        let id = e.mod_id.clone();
        self.errors.push(e);
        if disable {
            m.disabled = true;
            self.errors.push(ScriptError {
                mod_id: id,
                during: "dispatch".into(),
                kind: ErrorKind::Disabled,
                message: format!("disabled after {} errors", m.strikes),
            });
        }
    }
}

fn args_len(name: &str) -> usize {
    usize::from(!matches!(name, "on_load" | "on_reload"))
}

fn functions(ast: &AST) -> BTreeSet<(String, usize)> {
    ast.iter_functions()
        .map(|f| (f.name.to_string(), f.params.len()))
        .collect()
}

/// A Rhai engine without file, module, network, time or `eval` access, with size and depth
/// limits, an operation budget and the mod API.
fn sandboxed_engine(ctx: &Shared, limit: &Arc<AtomicU64>, used: &Arc<AtomicU64>) -> Engine {
    let mut e = Engine::new();
    e.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
    e.disable_symbol("eval");
    e.set_max_call_levels(32);
    e.set_max_expr_depths(64, 32);
    e.set_max_string_size(16 * 1024);
    e.set_max_array_size(10_000);
    e.set_max_map_size(10_000);
    e.set_max_modules(0);
    {
        let (limit, used) = (limit.clone(), used.clone());
        e.on_progress(move |ops| {
            used.store(ops, Ordering::Relaxed);
            (ops > limit.load(Ordering::Relaxed)).then(|| Dynamic::from("budget"))
        });
    }
    {
        let c = ctx.clone();
        e.on_print(move |s| lock(&c).log(s));
        let c = ctx.clone();
        e.on_debug(move |s, _, _| lock(&c).log(s));
    }
    register_api(&mut e, ctx);
    e
}

fn register_api(e: &mut Engine, ctx: &Shared) {
    type R<T> = Result<T, Box<EvalAltResult>>;
    let c = ctx.clone();
    e.register_fn("log", move |s: &str| lock(&c).log(s));

    // Deterministic randomness.
    let c = ctx.clone();
    e.register_fn("rand", move |lo: i64, hi: i64| lock(&c).rng.range(lo, hi));
    let c = ctx.clone();
    e.register_fn("rand_float", move || lock(&c).rng.next_f64());
    let c = ctx.clone();
    e.register_fn("chance", move |p: f64| lock(&c).rng.next_f64() < p);

    // World (read-only).
    let c = ctx.clone();
    e.register_fn("hero_pos", move || -> R<Dynamic> {
        let c = lock(&c);
        c.need(Capability::World)?;
        Ok(c.world.hero.to_dynamic())
    });
    let c = ctx.clone();
    e.register_fn("actor", move |id: i64| -> R<Dynamic> {
        let c = lock(&c);
        c.need(Capability::World)?;
        let Ok(id) = u32::try_from(id) else {
            return Ok(Dynamic::UNIT);
        };
        Ok(c.world
            .actors
            .get(&id)
            .map_or(Dynamic::UNIT, |a| a.to_dynamic()))
    });
    let c = ctx.clone();
    e.register_fn("actors", move || -> R<rhai::Array> {
        let c = lock(&c);
        c.need(Capability::World)?;
        Ok(c.world.actors.values().map(|a| a.to_dynamic()).collect())
    });
    let c = ctx.clone();
    e.register_fn("quest_status", move |q: &str| -> R<String> {
        let c = lock(&c);
        c.need(Capability::World)?;
        Ok(c.world.quests.get(q).cloned().unwrap_or_default())
    });
    let c = ctx.clone();
    e.register_fn("game_hours", move || -> R<f64> {
        let c = lock(&c);
        c.need(Capability::World)?;
        Ok(c.world.hours)
    });

    // Commands.
    fn pos(m: &Map) -> R<Vec3> {
        Vec3::from_map(m).ok_or_else(|| "position needs numeric x, y and z".into())
    }
    fn amount(n: i64) -> R<u32> {
        u32::try_from(n)
            .ok()
            .filter(|n| (1..=10_000).contains(n))
            .ok_or_else(|| format!("amount {n} out of range 1..=10000").into())
    }
    let c = ctx.clone();
    e.register_fn("spawn_npc", move |template: &str, at: Map| -> R<()> {
        let at = pos(&at)?;
        lock(&c).issue(
            Capability::Spawn,
            ScriptCommand::SpawnNpc {
                template: template.into(),
                at,
                yaw: 0.0,
            },
        )
    });
    let c = ctx.clone();
    e.register_fn(
        "spawn_npc",
        move |template: &str, at: Map, yaw: f64| -> R<()> {
            let at = pos(&at)?;
            lock(&c).issue(
                Capability::Spawn,
                ScriptCommand::SpawnNpc {
                    template: template.into(),
                    at,
                    yaw: yaw as f32,
                },
            )
        },
    );
    let c = ctx.clone();
    e.register_fn("give_hero", move |template: &str, n: i64| -> R<()> {
        let amount = amount(n)?;
        lock(&c).issue(
            Capability::Items,
            ScriptCommand::GiveHero {
                template: template.into(),
                amount,
            },
        )
    });
    let c = ctx.clone();
    e.register_fn(
        "give_npc",
        move |actor: i64, template: &str, n: i64| -> R<()> {
            let amount = amount(n)?;
            let actor = u32::try_from(actor).map_err(|_| format!("bad actor id {actor}"))?;
            lock(&c).issue(
                Capability::Items,
                ScriptCommand::GiveNpc {
                    actor,
                    template: template.into(),
                    amount,
                },
            )
        },
    );
    let c = ctx.clone();
    e.register_fn("notice", move |text: &str| -> R<()> {
        lock(&c).issue(Capability::Ui, ScriptCommand::Notice { text: text.into() })
    });
}
