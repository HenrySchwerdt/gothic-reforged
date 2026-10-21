#!/usr/bin/env python3
"""Merge the per-scenario dumps of the port-coverage build with the static manifest and write
the never-called list as markdown.

  python scripts/portcov/report.py --manifest .local/portcov/manifest.json \
      --runs .local/portcov/runs --states crates/script/src/routine/state.rs \
      --out docs/research/WIRING_COVERAGE.md
"""
import argparse, glob, json, os, re, collections

BT = chr(96)
NL = chr(10)


def code(s):
    return BT + s + BT


def summary(e):
    d = e.get('doc', '')
    d = re.sub(r'^\s*Port of\s+', '', d)
    m = re.search(r'\)\s*[:;,.]?\s*(.*)$', d)
    t = (m.group(1) if m and len(m.group(1)) > 12 else d).strip()
    t = t.replace('|', '/').replace(BT, '')
    return (t[:150] + '...') if len(t) > 150 else t


TABLE = [
    ('script/src/player/ranged', 'PS_Ranged* (bow, crossbow)'),
    ('script/src/player/melee', 'PS_Melee*'),
    ('script/src/player/magic', 'PS_Magic*'),
    ('script/src/player/normal', 'PS_Normal / Sneak / Swim'),
    ('script/src/player/evade', 'PS_Evade'),
    ('script/src/player/interact', 'PS_Interact*'),
    ('script/src/player/boulder', 'PS_Interact boulder'),
    ('script/src/player/craft', 'PS_Interact craft'),
    ('script/src/player/use_item', 'PS_HUD use item'),
    ('script/src/player/', 'PS_* other (player api / commands)'),
    ('script/src/combat/', 'ZS_Attack / combat AI'),
    ('script/src/magic/', 'spell effects / casting'),
    ('script/src/routine/', 'ZS_* routine tasks'),
    ('script/src/damage', 'damage pipeline'),
    ('script/src/assess', 'damage pipeline'),
    ('script/src/down', 'down / knock-out'),
    ('script/src/perception', 'perception / crime'),
    ('script/src/crime', 'perception / crime'),
    ('script/src/', 'script helpers / predicates / stats'),
    ('sim/', 'sim (stats, combat, AI, host)'),
    ('camera/', 'camera (PSCamera)'),
    ('input/', 'input / player control'),
    ('physics/', 'physics / movement'),
    ('projectile/', 'projectiles'),
    ('animation/', 'animation (EMotionFX wrapper, blending, attachments)'),
    ('gameplay/', 'gameplay (inventory, trade, mover, treasure, focus, stats)'),
    ('world/', 'world (entity admin, PVS, sectors, triggers, zones)'),
    ('audio/', 'audio sequencers'),
    ('ui/', 'ui'),
    ('fx/', 'fx / particles'),
    ('environment/', 'environment / weather'),
    ('render-rules/', 'render rules'),
    ('formats/', 'formats'),
    ('runtime/', 'runtime'),
    ('nav/', 'nav'),
]


def group_of(e):
    stem = re.sub(r'^crates/|^apps/', '', e['file'])
    for pre, g in TABLE:
        if stem.startswith(pre):
            return g
    return stem.split('/')[0]


def build_corpus(root):
    """crate dir -> list of production source texts (cfg(test) tails and comment lines dropped)."""
    corp = {}
    for top in ('crates', 'apps'):
        for dp, dn, fn in os.walk(os.path.join(root, top)):
            dn[:] = [d for d in dn if d not in ('target', 'tests', 'benches', 'examples')]
            for f in fn:
                if not f.endswith('.rs'):
                    continue
                path = os.path.join(dp, f)
                rel = os.path.relpath(path, root).replace(chr(92), '/')
                crate = '/'.join(rel.split('/')[:2])
                t = open(path, encoding='utf-8', errors='replace').read()
                m = re.search(r'#\[cfg\(test\)\]\s*mod\b', t)
                if m:
                    t = t[:m.start()]
                t = NL.join(l for l in t.split(NL) if not l.lstrip().startswith('//'))
                t = re.sub(r'gothic_portcov::hitf?!\([^;]*\);', '', t)
                corp.setdefault(crate, []).append(t)
    return corp


def refs(corp, name, own_crate):
    base = name.split('@')[0]
    pat = re.compile(r'(?<![\w])' + re.escape(base) + r'(?![\w])')
    defs = re.compile(r'fn\s+' + re.escape(base) + r'(?![\w])')
    inner = outer = ndefs = 0
    for crate, texts in corp.items():
        for t in texts:
            n = len(pat.findall(t))
            d = len(defs.findall(t))
            ndefs += d
            if crate == own_crate:
                inner += n - d
            else:
                outer += n
    return inner, outer, ndefs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--manifest', default='.local/portcov/manifest.json')
    ap.add_argument('--runs', default='.local/portcov/runs')
    ap.add_argument('--states', default='crates/script/src/routine/state.rs')
    ap.add_argument('--out', default='docs/research/WIRING_COVERAGE.md')
    ap.add_argument('--root', default='.')
    ap.add_argument('--head', default='scripts/portcov/wiring_head.md')
    ap.add_argument('--json', default='.local/portcov/merged.json')
    a = ap.parse_args()
    man = json.load(open(a.manifest))
    runs = {}
    for p in sorted(glob.glob(os.path.join(a.runs, '*.json'))):
        try:
            runs[os.path.basename(p)[:-5]] = json.load(open(p))
        except Exception:
            pass
    merged = collections.Counter()
    by_scn = collections.defaultdict(dict)
    for s, d in runs.items():
        for k, v in d.items():
            merged[k] += v
            if v:
                by_scn[k][s] = v
    fns = [e for e in man if e['kind'] == 'fn' and e['instrumented']]
    for e in fns:
        e['key'] = e['file'] + '#' + e['name']
        e['count'] = merged.get(e['key'], 0)
    corp = build_corpus(a.root)
    for e in fns:
        own = '/'.join(e['file'].split('/')[:2])
        e['inner'], e['outer'], e['ndefs'] = refs(corp, e['name'], own)
        if e['count']:
            e['status'] = 'called'
        elif e['inner'] + e['outer'] == 0:
            e['status'] = 'dead'
        elif e['outer'] == 0:
            e['status'] = 'internal-only'
        else:
            e['status'] = 'wired-not-exercised'
        if e['ndefs'] > 1 and e['status'] != 'called':
            e['status'] += ' (name shared by %d fns: heuristic)' % e['ndefs']
    called = [e for e in fns if e['count']]
    never = [e for e in fns if not e['count']]
    noinst = [e for e in man if e['kind'] == 'fn' and not e['instrumented']]
    other = [e for e in man if e['kind'] != 'fn']
    st_src = open(a.states, encoding='utf-8').read()
    states = re.findall(r'=>\s*"((?:PS|ZS|BS)_\w+)"', st_src)
    st_hit = {s: merged.get('state:' + s, 0) for s in states}
    json.dump(dict(runs=sorted(runs), counts=dict(merged)), open(a.json, 'w'))
    o = []
    w = o.append
    head = open(a.head, encoding='utf-8').read() if a.head and os.path.exists(a.head) else None
    if head:
        w(head)
    else:
        w('# Wiring coverage: ported original functions vs. the running game' + NL)
    w('Generated by `scripts/portcov/` (crate `gothic-portcov`, cargo feature `port-coverage` of `gothic-reforged`). '
      '`scripts/portcov/instrument.py` inserts `gothic_portcov::hit!("<fn>")` at the top of every function whose doc comment says '
      '`Port of <Name> (<Dll>:<addr>)`; the macro expands to nothing without the feature. Each QA scenario was run as '
      '`gothic-reforged --play --qa <scenario>` with the feature on, and the call counts of all runs were merged '
      '(`scripts/portcov/run_all.sh`, `scripts/portcov/report.py`). **Called** means executed at least once by the running '
      'game in any scenario (unit tests do not count; a function reached only from another never-used function counts as '
      'never called only if it is itself never executed). Scenarios run: ' + ', '.join(code(s) for s in sorted(runs)) + '.\n')
    w('## Totals\n')
    w('* Doc blocks with `Port of`: %d; instrumentable functions: %d (+%d not instrumented, see the end); other items (tables, consts, structs, impls; not countable): %d.' % (len(man), len(fns), len(noinst), len(other)))
    w('* **Called in play: %d**, **never called: %d** (%d%% of instrumented functions).' % (len(called), len(never), 100 * len(never) // max(1, len(fns))))
    stc = collections.Counter(e['status'].split(' ')[0] for e in never)
    w('* Of the never-called: **%d DEAD** (no production reference anywhere: nobody could call them), **%d internal-only** (referenced only inside their own crate, entry point never reached), **%d wired-not-exercised** (referenced from another crate, but no QA scenario reaches the call; long-running routines, rare events). A trailing "?" marks names shared by several functions (reference count is a name match, so the status is a heuristic).' % (stc['dead'], stc['internal-only'], stc['wired-not-exercised']))
    live = [s for s, c in st_hit.items() if c]
    w('* Script states (ZS_/PS_/BS_, data tables interpreted by `run_state`): %d of %d entered at least once.\n' % (len(live), len(states)))
    grp = collections.defaultdict(lambda: [0, 0])
    for e in fns:
        grp[group_of(e)][0 if e['count'] else 1] += 1
    w('## By subsystem\n')
    w('| Subsystem / original state family | called | never called |')
    w('| --- | ---: | ---: |')
    for g, (c, n) in sorted(grp.items(), key=lambda x: -x[1][1]):
        w('| %s | %d | %d |' % (g, c, n))
    w('')
    w('## Script states never entered in play\n')
    fam = collections.defaultdict(list)
    for s, c in st_hit.items():
        if not c:
            fam[s.split('_')[0]].append(s)
    for f in ('PS', 'ZS', 'BS'):
        if fam.get(f):
            tot = sum(1 for s in states if s.startswith(f + '_'))
            w('**%s_** (%d of %d): ' % (f, len(fam[f]), tot) + ', '.join(code(s) for s in fam[f]) + '\n')
    w('Entered: ' + ', '.join('%s (%d)' % (code(s), c) for s, c in sorted(st_hit.items()) if c) + '\n')
    w('## Ported functions never called in play (by subsystem)\n')
    bg = collections.defaultdict(list)
    for e in never:
        bg[group_of(e)].append(e)
    for g, es in sorted(bg.items(), key=lambda x: -len(x[1])):
        w('### %s (%d)\n' % (g, len(es)))
        w('| function | original (Dll:addr) | what the original does | wiring | file |')
        w('| --- | --- | --- | --- | --- |')
        for e in sorted(es, key=lambda e: (e['file'], e['line'])):
            ad = ', '.join(e['addrs'][:2]) or '-'
            port = e['port'].replace('|', '/')[:50].replace(BT, '')
            w('| %s (%s) | %s | %s | %s | %s:%d |' % (code(e['name']), port, ad, summary(e), e['status'].split(' ')[0].upper() + (' ?' if '(' in e['status'] else ''), e['file'], e['line']))
        w('')
    w('## Ported functions called in play (calls over all scenarios)\n')
    w('| function | calls | scenarios | file |')
    w('| --- | ---: | --- | --- |')
    for e in sorted(called, key=lambda e: -e['count']):
        sc = ', '.join(sorted(by_scn[e['key']]))[:70]
        w('| %s | %d | %s | %s:%d |' % (code(e['name']), e['count'], sc, e['file'], e['line']))
    w('')
    if noinst:
        w('## Not instrumented (files with uncommitted work in progress)\n')
        for e in noinst:
            w('* %s %s:%d' % (code(e['name'] or '?'), e['file'], e['line']))
        w('')
    open(a.out, 'w', encoding='utf-8').write('\n'.join(o))
    print(len(fns), 'fns;', len(called), 'called;', len(never), 'never; states', len(live), '/', len(states))


main()
