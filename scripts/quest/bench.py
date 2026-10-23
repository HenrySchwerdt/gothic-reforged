"""Run the Quest app with debug.gr.* knobs and print steady-state stats. Usage: bench.py name key=val [key=val ...]"""
import subprocess, sys, re, time, os
os.environ["MSYS_NO_PATHCONV"] = "1"
D = os.environ.get("QUEST_SERIAL", "2G0YC1ZG1003WN")
PKG = "dev.gothicreforged.questtest"
def adb(cmd):
    return subprocess.run(f'npx -y metavr adb -d {D} shell "{cmd}"', capture_output=True, text=True, shell=True).stdout
name = sys.argv[1]
knobs = dict(a.split("=", 1) for a in sys.argv[2:])
for k in ["greybox", "res_scale", "radius", "lod_scale", "cull_scale", "sun", "noindirect", "msaa", "weapons", "hero", "orcs_on", "orcs", "mirror", "unlit_world", "phys", "autowalk", "start_yaw", "cheap", "veg", "veg_density", "veg_far_density", "clips", "pd_lin_hz", "pd_lin_damp", "pd_ang_hz", "pd_ang_damp", "strength", "weapon_mass", "hit_min_speed", "hit_full_speed", "hit_impulse", "stagger_impulse", "fall_impulse", "stagger_strength", "stagger_time", "orc_substeps", "orc_iter"]:
    adb(f"setprop debug.gr.{k} ''")
for k, v in knobs.items():
    adb(f"setprop debug.gr.{k} {v}")
adb("wm dismiss-keyguard; input keyevent KEYCODE_WAKEUP")
subprocess.run(f"npx -y metavr device wake -d {D}", capture_output=True, shell=True)
time.sleep(2)
adb("logcat -c")
subprocess.run(f"npx -y metavr app launch -d {D} --cold-start {PKG}", capture_output=True, shell=True)
time.sleep(float(os.environ.get("WAIT", "22")))
log = adb("logcat -d -v time | grep -E 'xr-stats|scene\]' | grep RustStdout")
fps = [float(m) for m in re.findall(r"fps=([\d.]+)", log)][-6:]
upd = [float(m) for m in re.findall(r"update=([\d.]+)ms", log)][-6:]
wait = [float(m) for m in re.findall(r"wait=([\d.]+)ms", log)][-6:]
sc = re.findall(r"\[scene\] (.*)", log)
prof=re.findall(r"fixed=([\d.]+)ms/frame steps=([\d.]+)/frame main=([\d.]+)ms", log)[-4:]
pf=[tuple(map(float,x)) for x in prof]
pr=" fixed=%.2f steps=%.2f main=%.2f"%tuple(sum(c)/max(1,len(pf)) for c in zip(*pf)) if pf else ""
print(f"{name}{pr}: fps={sum(fps)/max(1,len(fps)):.1f} update={sum(upd)/max(1,len(upd)):.2f}ms wait={sum(wait)/max(1,len(wait)):.2f}ms | {sc[-1][:200] if sc else ''}")
