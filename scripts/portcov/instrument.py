#!/usr/bin/env python3
"""Mechanically insert `gothic_portcov::hit!("<fn>");` at the top of every function whose doc
comment contains `Port of <Name> (<Dll>:<addr>)`, and write a static manifest of all such
doc blocks (functions, const fns, tables, structs ...).

  python scripts/portcov/instrument.py [--root DIR] [--manifest FILE] [--skip PATH ...]
                                       [--skip-dirty] [--dry]

Idempotent. `--skip-dirty` leaves files with uncommitted changes (git status) alone, so concurrent
work in progress is not touched; they are still listed in the manifest (`instrumented: false`).
Zero cost when the `port-coverage` feature of the game is off: the macro expands to nothing.
"""
import argparse, json, os, re, subprocess, sys
BS = chr(92)

ITEM = re.compile(r'^\s*(pub(\([^)]*\))?\s+)?(default\s+)?(const\s+)?(async\s+)?(unsafe\s+)?(extern\s+"[^"]*"\s+)?fn\s+(\w+)')
ADDR = re.compile(r'\b([A-Za-z_]\w*):(0x[0-9a-fA-F]+|[0-9a-f]{5,6})(?![\w:])')

def find_body(text, pos):
    """Index of the `{` opening the fn body starting the search at pos, or -1 (declaration)."""
    depth = 0
    i = pos
    n = len(text)
    while i < n:
        c = text[i]
        if c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == BS else 1
        elif c == '/' and text.startswith('//', i):
            while i < n and text[i] != '\n':
                i += 1
        elif c in '([':
            depth += 1
        elif c in ')]':
            depth -= 1
        elif depth == 0 and c == ';':
            return -1
        elif depth == 0 and c == '{':
            return i
        i += 1
    return -1

def process(path, rel, write, skip):
    text = open(path, encoding='utf-8').read()
    lines = text.split('\n')
    offs = [0]
    for l in lines:
        offs.append(offs[-1] + len(l) + 1)
    entries, inserts = [], []
    seen_names = {}
    i = 0
    while i < len(lines):
        s = lines[i].lstrip()
        if not s.startswith('///'):
            i += 1
            continue
        j = i
        while j < len(lines) and lines[j].lstrip().startswith('///'):
            j += 1
        block = ' '.join(l.lstrip()[3:].strip() for l in lines[i:j])
        k = j
        if 'Port of' in block:
            # skip attributes
            while k < len(lines) and (lines[k].lstrip().startswith('#[') or lines[k].strip() == ''):
                k += 1
                if lines[k - 1].strip() == '':
                    break
            m = ITEM.match(lines[k]) if k < len(lines) else None
            pm = re.search(r'Port of (.*?)(?:\(|:|$)', block)
            port_name = re.sub(r'[`\s]+$|^[`\s]+', '', pm.group(1)) if pm else ''
            addrs = sorted({f'{a}:{b}' for a, b in ADDR.findall(block)})
            e = dict(file=rel, line=i + 1, port=port_name[:90], addrs=addrs,
                     item=lines[k].strip()[:80] if k < len(lines) else '', kind='other', doc=block[:400],
                     name=None, instrumented=False)
            if m:
                name = m.group(8)
                n = seen_names.get(name, 0) + 1
                seen_names[name] = n
                key = name if n == 1 else f'{name}@{n}'
                e['name'] = key
                if m.group(4):
                    e['kind'] = 'const-fn'
                else:
                    body = find_body(text, offs[k] + m.end())
                    if body < 0:
                        e['kind'] = 'decl'
                    else:
                        e['kind'] = 'fn'
                        marker = f'gothic_portcov::hit!("{key}");'
                        after = text[body + 1: body + 1 + 200]
                        if marker in after.split('\n', 2)[1] if '\n' in after else False:
                            e['instrumented'] = True
                        elif skip:
                            pass
                        else:
                            indent = re.match(r'\s*', lines[k]).group(0) + '    '
                            inserts.append((body + 1, f'\n{indent}{marker}'))
                            e['instrumented'] = True
            entries.append(e)
        i = j
    if inserts and write:
        for pos, ins in sorted(inserts, reverse=True):
            text = text[:pos] + ins + text[pos:]
        open(path, 'w', encoding='utf-8', newline='').write(text)
    return entries, bool(inserts)

def ensure_dep(root, crate_dir, write):
    cargo = os.path.join(root, crate_dir, 'Cargo.toml')
    t = open(cargo, encoding='utf-8').read()
    if 'gothic-portcov' in t:
        return
    up = '../' * (crate_dir.count('/') ) + 'crates/portcov' if crate_dir.startswith('apps') else '../portcov'
    line = f'gothic-portcov = {{ path = "{up}" }}\n'
    if '[dependencies]' in t:
        t = t.replace('[dependencies]\n', '[dependencies]\n' + line, 1)
    else:
        t += '\n[dependencies]\n' + line
    if write:
        open(cargo, 'w', encoding='utf-8', newline='').write(t)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--root', default='.')
    ap.add_argument('--manifest', default='.local/portcov/manifest.json')
    ap.add_argument('--skip', nargs='*', default=[])
    ap.add_argument('--skip-dirty', action='store_true')
    ap.add_argument('--dry', action='store_true')
    a = ap.parse_args()
    root = a.root
    skip = {s.replace(BS, '/') for s in a.skip}
    if a.skip_dirty:
        out = subprocess.run(['git', 'status', '--porcelain', '-uall'], cwd=root, capture_output=True, text=True).stdout
        for l in out.splitlines():
            skip.add(l[3:].strip().strip('"').replace(BS, '/'))
    manifest, touched = [], set()
    for top in ('crates', 'apps'):
        for dp, dn, fn in os.walk(os.path.join(root, top)):
            dn[:] = [d for d in dn if d not in ('target', 'portcov', 'tests', 'examples', 'benches')]
            for f in fn:
                if not f.endswith('.rs'):
                    continue
                path = os.path.join(dp, f)
                rel = os.path.relpath(path, root).replace(BS, '/')
                ents, changed = process(path, rel, not a.dry and rel not in skip, rel in skip)
                if rel in skip:
                    for e in ents:
                        e['skipped_dirty'] = True
                manifest += ents
                if changed:
                    touched.add('/'.join(rel.split('/')[:2]))
    for c in sorted(touched):
        ensure_dep(root, c, not a.dry)
    os.makedirs(os.path.dirname(os.path.join(root, a.manifest)), exist_ok=True)
    json.dump(manifest, open(os.path.join(root, a.manifest), 'w'), indent=1)
    kinds = {}
    for e in manifest:
        kinds[e['kind']] = kinds.get(e['kind'], 0) + 1
    print('doc blocks:', len(manifest), kinds, 'instrumented:', sum(e['instrumented'] for e in manifest),
          'crates with new dep:', sorted(touched))

main()
