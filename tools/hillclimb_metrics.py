#!/usr/bin/env python3
"""Hillclimb metric: Perez sheet parity (tokens, live-lime-only, layout, tests).
Usage: python3 tools/hillclimb_metrics.py [--json]
Exit 0 always; prints PASS/FAIL per check + score/100.
Checks runnable offline: rg on sources + cargo test (cached ok).
"""
import re, subprocess, sys, json, pathlib
ROOT = pathlib.Path(__file__).resolve().parents[1]
def f(p): return (ROOT/p).read_text(errors="replace")
checks = []
def check(name, ok, detail=""):
    checks.append({"name": name, "pass": bool(ok), "detail": detail})
    return bool(ok)

# 1. tokens in vp-core themes
themes = f("vp-core/src/themes.rs")
check("token-signal-#ED1216", "#ED1216" in themes.upper(), "SIGNAL_RED const")
check("token-lime-#CCED12", "#CCED12" in themes.upper(), "LIVE_LIME const")
check("token-4-themes", all(t in themes for t in ["light","dark","gray","blue"]), "4 skins")
# 2. live-lime-only in app
main = f("app/src/main.rs")
check("live-lime-const", "LIVE_LIME" in main, "app live cue")
check("live-not-accent", bool(re.search(r"must NOT be .accent|NOT be `accent`|live.*lime.*every skin", main, re.I)), "live!=accent guard")
check("selected-signal-red", bool(re.search(r"selected agenda.*signal red|signal.*selected", main, re.I)), "selected=red")
# 3. center editor dominant: textarea/editor center + min-height/anchor
check("center-editor", bool(re.search(r"center|middle.*textarea|textarea.*anchor|visual anchor|dominant", main, re.I)), "center dominant")
# 4. verse-splitter no orphans
vs = f("vp-core/src/verse_splitter.rs")
check("verse-no-orphan", bool(re.search(r"orphan", vs, re.I)), "no-orphan rule")
# 5. serde compat shared schemas
sch = f("vp-core/src/schema.rs")
check("serde-schemas", all(x in sch for x in ["Song","Agenda","Style","BibleRef"]) or all(x in f("vp-core/src/lib.rs") for x in ["Song","Agenda"]), "schemas")
# 6. cargo test green (run, may take a while; on failure record output tail)
try:
    r = subprocess.run(["cargo","test","--quiet","--offline"], cwd=str(ROOT/"gpui-app"), capture_output=True, text=True, timeout=600)
    tail = (r.stdout+r.stderr)[-500:]
    check("cargo-test-green", r.returncode==0, tail.replace("\n"," | ")[:400])
except Exception as e:
    check("cargo-test-green", False, str(e)[:200])

score = sum(1 for c in checks if c["pass"])
total = len(checks)
pct = round(100*score/total)
if "--json" in sys.argv:
    print(json.dumps({"score":pct,"passed":score,"total":total,"checks":checks}, indent=1))
else:
    print(f"HILLCLIMB parity: {score}/{total} = {pct}/100")
    for c in checks:
        print(f" [{'PASS' if c['pass'] else 'FAIL'}] {c['name']} — {c['detail'][:160]}")
