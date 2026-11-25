"""Builds report figures (SVG) from the Cognoscenti results JSON.

Usage: python3 make_figures.py <results-dir> <out-dir>
Needs a Memory Trustworthiness scorer module `mt_score` on PYTHONPATH (same file as docs).
"""
import json, math, sys, os
sys.path.insert(0, os.path.dirname(__file__))
from mt_score import load, score

RES, OUT = sys.argv[1], sys.argv[2]
FONT = "Helvetica Neue, Helvetica, Arial, sans-serif"
BLUE, GREEN, YELLOW, RED = "#2D6BFF", "#2E9E4F", "#FFD400", "#E5383B"
INK, MUTED, GRID, GREY = "#111827", "#6B7280", "#E5E7EB", "#9CA3AF"

arm2 = load(f"{RES}/mt-b3kw.results.json")["systems"][0]
arm1 = load(f"{RES}/mt-b2def.results.json")["systems"][0]
base = {s["system"]: s for s in load(f"{RES}/baselines-r5.results.json")["systems"]}
S = {"Achiral": score(arm2)}
ORDER = ["bm25", "recency", "verbatim", "bounded-window", "full-context"]
for k in ORDER:
    S[k] = score(base[k])
A1 = score(arm1)
COMP = ["Access", "Currency", "Recall", "Private memory"]
json.dump({"Achiral": S["Achiral"], "Achiral arm 1": A1, **{k: S[k] for k in ORDER}},
          open(f"{OUT}/scores.json", "w"), indent=1)

def svg(w, h, body):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" '
            f'font-family="{FONT}" font-size="11" fill="{INK}"><rect width="{w}" height="{h}" fill="white"/>{body}</svg>')

def t(x, y, s, size=11, anchor="start", fill=INK, weight="400"):
    s = s.replace("&", "&amp;")
    return f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" text-anchor="{anchor}" fill="{fill}" font-weight="{weight}">{s}</text>'

def line(x1, y1, x2, y2, c=GRID, w=1, dash=""):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    return f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{x2:.1f}" y2="{y2:.1f}" stroke="{c}" stroke-width="{w}"{d}/>'

def rect(x, y, w, h, c):
    return f'<rect x="{x:.1f}" y="{y:.1f}" width="{max(w,0):.1f}" height="{h:.1f}" fill="{c}"/>'

def col(name):
    return BLUE if name == "Achiral" else GREY

def hbars(rows, w, left, top, bh, gap, xmax, fmt, ticks, pw):
    """rows: (label, value, lo, hi, color, note)."""
    b = []
    for tk in ticks:
        x = left + tk / xmax * pw
        b.append(line(x, top - 6, x, top + len(rows) * (bh + gap), GRID))
        b.append(t(x, top - 12, fmt(tk), 10, "middle", MUTED))
    for i, (lab, v, lo, hi, c, note) in enumerate(rows):
        y = top + i * (bh + gap)
        b.append(t(left - 10, y + bh * 0.72, lab, 12, "end", INK, "600" if c == BLUE else "400"))
        b.append(rect(left, y, v / xmax * pw, bh, c))
        if lo is not None:
            xl, xh, ym = left + lo / xmax * pw, left + hi / xmax * pw, y + bh / 2
            b.append(line(xl, ym, xh, ym, INK, 1.4) + line(xl, ym - 4, xl, ym + 4, INK, 1.4) + line(xh, ym - 4, xh, ym + 4, INK, 1.4))
        b.append(t(left + max(v, hi or 0) / xmax * pw + 8, y + bh * 0.72, note, 11, "start", INK))
    return "".join(b)

# 1. headline score
names = ["Achiral"] + ORDER
rows = []
for n in names:
    o = S[n]; lo, hi = o["score_ci"]
    rows.append((n, o["score"], lo, hi, col(n), f'{o["score"]:.2f}'))
rows.sort(key=lambda r: -r[1])
w, bh, gap, top, left, pw = 680, 26, 12, 46, 130, 430
h = top + len(rows) * (bh + gap) + 34
b = hbars(rows, w, left, top, bh, gap, 1.0, lambda v: f"{v:.1f}", [0, .2, .4, .6, .8, 1.0], pw)
b += t(left, h - 8, "Bars: Memory Trustworthiness. Whiskers: 95% bootstrap interval over questions. 544 questions, 5 replicates.", 10, "start", MUTED)
open(f"{OUT}/fig1_score.svg", "w").write(svg(w, h, b))

# 2. components small multiples
pw2, ph = 150, 150
w, h = 4 * (pw2 + 36) + 60, ph + 150
b = ""
for ci, c in enumerate(COMP):
    x0 = 56 + ci * (pw2 + 36)
    b += t(x0, 34, f'{c}', 13, "start", INK, "700") + t(x0, 49, f'n = {S["Achiral"]["n"][c]}', 10, "start", MUTED)
    for tk in (0, .5, 1):
        y = 64 + ph - tk * ph
        b += line(x0, y, x0 + pw2, y, GRID) + (t(x0 - 6, y + 3, f"{tk:g}", 9, "end", MUTED) if ci == 0 else "")
    bw = pw2 / len(names)
    for i, n in enumerate(names):
        v = S[n][c]; lo, hi = S[n]["ci"][c]
        x = x0 + i * bw + 3
        b += rect(x, 64 + ph - v * ph, bw - 6, v * ph, col(n))
        if hi - lo > 1e-9:
            xm = x + (bw - 6) / 2
            b += line(xm, 64 + ph - hi * ph, xm, 64 + ph - lo * ph, INK, 1.2)
        b += t(x + (bw-6)/2, 64 + ph - v * ph - (10 if hi - lo > 1e-9 else 4) - 2 if False else 64 + ph - max(v, hi) * ph - 4, f"{v:.2f}", 7, "middle", INK)
        b += f'<text x="{x + (bw-6)/2:.1f}" y="{64+ph+12}" font-size="8" fill="{MUTED}" text-anchor="end" transform="rotate(-50 {x + (bw-6)/2:.1f} {64+ph+12})">{n}</text>'
open(f"{OUT}/fig2_components.svg", "w").write(svg(w, h, b))

# 3. scope: leak rate
rows = []
for n in names:
    leak = 1 - S[n]["Access"]; lo, hi = S[n]["ci"]["Access"]
    if n == "Achiral":
        rows.append((n, 0.0, None, None, BLUE, "0 of 141 (below 2.1% by the 3/n bound)"))
    else:
        rows.append((n, leak, 1 - hi, 1 - lo, GREY, f'{S[n]["leak_events"]} of 141 ({leak*100:.0f}%)'))
rows.sort(key=lambda r: r[1])
w, bh, gap, top, left, pw = 680, 26, 12, 46, 130, 300
h = top + len(rows) * (bh + gap) + 34
b = hbars(rows, w, left, top, bh, gap, 1.0, lambda v: f"{v*100:.0f}%", [0, .25, .5, .75, 1.0], pw)
b += rect(left, top + 0 * (bh + gap), 0.021 * pw, bh, BLUE)
b += t(left, h - 8, "Share of 141 scope and agent trials in which a private note was returned to someone who should not see it.", 10, "start", MUTED)
open(f"{OUT}/fig3_scope.svg", "w").write(svg(w, h, b))

# 4. stability: per-replicate fully recalled, both arms
def reps(s): return [r["fully_recalled"] for r in s["replicates"]]
r1, r2 = reps(arm1), reps(arm2)
w, h = 680, 250
left, top, pw, ph = 70, 40, 560, 140
lo_, hi_ = 400, 500
b = ""
for tk in range(400, 501, 25):
    y = top + ph - (tk - lo_) / (hi_ - lo_) * ph
    b += line(left, y, left + pw, y) + t(left - 8, y + 3, str(tk), 10, "end", MUTED)
for i in range(5):
    x = left + 60 + i * 110
    b += t(x, top + ph + 18, f"replicate {i}", 10, "middle", MUTED)
    for v, c, dx in ((r1[i], GREY, -14), (r2[i], BLUE, 14)):
        y = top + ph - (v - lo_) / (hi_ - lo_) * ph
        b += f'<circle cx="{x+dx}" cy="{y:.1f}" r="5" fill="{c}"/>' + t(x + dx, y - 9, str(v), 10, "middle", INK)
b += f'<circle cx="{left}" cy="{h-34}" r="5" fill="{GREY}"/>' + t(left + 10, h - 30, "arm 1 (retrieval without keyword candidates)", 10)
b += f'<circle cx="{left+290}" cy="{h-34}" r="5" fill="{BLUE}"/>' + t(left + 300, h - 30, "arm 2 (with keyword candidates)", 10)
fl = arm2["flips"]
b += t(left, h - 8, f'Questions fully recalled out of 544. Flip rate, arm 2: {fl["events"]} of 544 ({fl["estimate"]*100:.1f}%, 95% interval {fl["ci"][0]*100:.1f}% to {fl["ci"][1]*100:.1f}%).', 10, "start", MUTED)
open(f"{OUT}/fig4_stability.svg", "w").write(svg(w, h, b))

# 5. calibration (derived)
bins = [x for x in arm2["calibration"]["bins"] if x["count"]]
w, h, s0, left, top = 360, 360, 250, 70, 24
b = line(left, top + s0, left + s0, top + s0, INK) + line(left, top, left, top + s0, INK)
for tk in (0, .25, .5, .75, 1):
    x = left + tk * s0; y = top + s0 - tk * s0
    b += t(x, top + s0 + 16, f"{tk:g}", 10, "middle", MUTED) + t(left - 8, y + 3, f"{tk:g}", 10, "end", MUTED)
b += line(left, top + s0, left + s0, top, GREY, 1.2, "4 3")
for x in bins:
    b += f'<circle cx="{left + x["predicted_mean"]*s0:.1f}" cy="{top + s0 - x["observed_frequency"]*s0:.1f}" r="{4 + math.sqrt(x["count"])/6:.1f}" fill="{BLUE}" fill-opacity="0.8"/>'
b += t(left + s0 / 2, top + s0 + 34, "Predicted probability (derived from ranking)", 11, "middle", MUTED)
b += f'<text x="18" y="{top+s0/2}" font-size="11" fill="{MUTED}" text-anchor="middle" transform="rotate(-90 18 {top+s0/2})">Observed frequency</text>'
open(f"{OUT}/fig5_calibration.svg", "w").write(svg(w, h, b))

# 6. tokens: bars (log) + trust vs tokens scatter
tok = {"Achiral": arm2["tokens"]}
for k in ORDER: tok[k] = base[k]["tokens"]
w, h = 740, 330
b = t(0, 16, "Tokens injected per question (words, log scale)", 12, "start", INK, "700")
left, top, pw, bh, gap = 130, 56, 300, 22, 10
lg = lambda v: (math.log10(v) - 0) / 4.2 * pw
for tk in (1, 10, 100, 1000, 10000):
    x = left + lg(tk)
    b += line(x, top - 6, x, top + 6 * (bh + gap)) + t(x, top - 12, f"{tk:,}", 10, "middle", MUTED)
rows = sorted(tok.items(), key=lambda kv: kv[1]["estimate"])
for i, (n, v) in enumerate(rows):
    y = top + i * (bh + gap)
    b += t(left - 10, y + 16, n, 12, "end", INK, "600" if n == "Achiral" else "400")
    b += rect(left, y, lg(v["estimate"]), bh, col(n))
    b += t(left + lg(v["estimate"]) + 8, y + 16, f'{v["estimate"]:,.1f}', 11)
# scatter
sx, sy, sw, sh = 510, 70, 190, 190
b += t(sx - 20, 40, "Trust against tokens", 12, "start", INK, "700")
b += line(sx, sy + sh, sx + sw, sy + sh, INK) + line(sx, sy, sx, sy + sh, INK)
for tk in (10, 100, 1000, 10000):
    x = sx + (math.log10(tk) - 1) / 3.2 * sw
    b += t(x, sy + sh + 14, f"{tk:,}", 9, "middle", MUTED)
for tk in (0, .5, 1):
    y = sy + sh - tk * sh
    b += t(sx - 6, y + 3, f"{tk:g}", 9, "end", MUTED) + line(sx, y, sx + sw, y, GRID)
for n in names:
    x = sx + (math.log10(tok[n]["estimate"]) - 1) / 3.2 * sw
    y = sy + sh - S[n]["score"] * sh
    dx, dy, an = {"Achiral": (10, -2, "start"), "bm25": (10, 3, "start"), "recency": (10, -3, "start"), "verbatim": (10, 12, "start"), "bounded-window": (0, -12, "middle"), "full-context": (0, -12, "end")}[n]
    b += f'<circle cx="{x:.1f}" cy="{y:.1f}" r="5.5" fill="{col(n)}"/>' + t(x + dx, y + dy, n, 9, an, INK)
b += t(sx + sw / 2, sy + sh + 32, "Tokens per question", 10, "middle", MUTED)
b += f'<text x="{sx-34}" y="{sy+sh/2}" font-size="10" fill="{MUTED}" text-anchor="middle" transform="rotate(-90 {sx-34} {sy+sh/2})">Memory Trustworthiness</text>'
b += t(0, h - 8, "Tokens are whitespace-separated words on the read path only; what ingestion costs is not measured.", 10, "start", MUTED)
open(f"{OUT}/fig6_tokens.svg", "w").write(svg(w, h, b))

# 7. arms by component
w, h = 680, 270
left, top, pw, ph = 60, 36, 580, 170
b = ""
for tk in (0, .25, .5, .75, 1):
    y = top + ph - tk * ph
    b += line(left, y, left + pw, y) + t(left - 8, y + 3, f"{tk:g}", 10, "end", MUTED)
items = COMP + ["Trustworthiness"]
gw = pw / len(items)
for i, c in enumerate(items):
    for j, (o, colr) in enumerate(((A1, GREY), (S["Achiral"], BLUE))):
        v = o["score"] if c == "Trustworthiness" else o[c]
        lo, hi = o["score_ci"] if c == "Trustworthiness" else o["ci"][c]
        x = left + i * gw + 24 + j * 52
        b += rect(x, top + ph - v * ph, 44, v * ph, colr) + t(x + 22, top + ph - v * ph - 14 if hi - lo > 1e-9 else top + ph - v * ph - 5, f"{v:.2f}", 10, "middle")
        if hi - lo > 1e-9:
            xm = x + 22
            b += line(xm, top + ph - hi * ph, xm, top + ph - lo * ph, INK, 1.2)
    b += t(left + i * gw + gw / 2, top + ph + 18, c, 11, "middle", INK, "700" if c == "Trustworthiness" else "400")
b += f'<rect x="{left}" y="{h-34}" width="10" height="10" fill="{GREY}"/>' + t(left + 16, h - 25, "Arm 1: 0.80", 10)
b += f'<rect x="{left+110}" y="{h-34}" width="10" height="10" fill="{BLUE}"/>' + t(left + 126, h - 25, "Arm 2: 0.88 (keyword candidates added)", 10)
open(f"{OUT}/fig7_arms.svg", "w").write(svg(w, h, b))

# 8. recall by category, arm1 vs arm2
def cats(s): return {c["name"]: (c["estimate"], c["n"]) for c in s["categories"]}
c1, c2 = cats(arm1), cats(arm2)
order = ["scope", "supersede", "paraphrase", "retrieve", "stable", "rephrase", "owner", "agent", "individual"]
w, bh, gap, top, left, pw = 680, 11, 7, 40, 130, 380
h = top + len(order) * (2 * bh + gap + 8) + 40
b = ""
for tk in (0, .25, .5, .75, 1):
    x = left + tk * pw
    b += line(x, top - 6, x, h - 40) + t(x, top - 12, f"{tk:g}", 10, "middle", MUTED)
for i, c in enumerate(order):
    y = top + i * (2 * bh + gap + 8)
    b += t(left - 10, y + bh + 4, f"{c} (n={c2[c][1]})", 12, "end")
    b += rect(left, y, c1[c][0] * pw, bh, GREY) + t(left + c1[c][0] * pw + 6, y + bh - 2, f"{c1[c][0]:.2f}", 10)
    b += rect(left, y + bh + 1, c2[c][0] * pw, bh, BLUE) + t(left + c2[c][0] * pw + 6, y + 2 * bh - 1, f"{c2[c][0]:.2f}", 10)
b += f'<rect x="{left}" y="{h-26}" width="10" height="10" fill="{GREY}"/>' + t(left + 16, h - 17, "Arm 1", 10)
b += f'<rect x="{left+80}" y="{h-26}" width="10" height="10" fill="{BLUE}"/>' + t(left + 96, h - 17, "Arm 2   (mean recall@3 per category, 5 replicates)", 10)
open(f"{OUT}/fig8_categories.svg", "w").write(svg(w, h, b))
print("ok")
