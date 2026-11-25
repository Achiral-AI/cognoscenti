"""Paired comparison of two Memory Trustworthiness runs on the same stream (Plan 45).

Usage: python3 mt_compare.py <control.results.json> <treatment.results.json>

Questions are matched by id. For each of Currency, Recall and Private memory the
difference (treatment minus control) of the mean per-question recall is bootstrapped
over matched questions (10,000 draws, 95% percentile interval). Access is compared
by leak counts: any leak in either run is reported, not bootstrapped.
Prints the pre-registered checks H1 (Private memory gain >= 0.05, interval above 0),
H2 (no leaks) and H3 (Recall and Currency not more than 0.02 lower).
"""
import json, random, sys
from mt_score import POOLS

def load(path):
    s = json.load(open(path))["systems"][0]
    return s, {q["id"]: q for q in s["questions"]}

def leaks(s):
    return {l["name"]: (l["events"] or 0, l["n"]) for l in s["leaks"]}

def boot(diffs, draws=10000, seed=1):
    rng = random.Random(seed)
    n = len(diffs)
    out = sorted(sum(rng.choice(diffs) for _ in range(n)) / n for _ in range(draws))
    return sum(diffs) / n, out[int(0.025 * draws)], out[int(0.975 * draws) - 1]

def main(a, b):
    sa, qa = load(a)
    sb, qb = load(b)
    shared = sorted(set(qa) & set(qb))
    if len(shared) != len(qa) or len(shared) != len(qb):
        print(f"warning: {len(shared)} matched of {len(qa)} and {len(qb)} questions")
    res = {}
    for name, cats in POOLS.items():
        ids = [i for i in shared if qa[i]["category"] in cats]
        res[name] = boot([qb[i]["recall"] - qa[i]["recall"] for i in ids]) + (len(ids),)
        d, lo, hi = res[name][:3]
        print(f"{name:15} n={len(ids):3} diff {d:+.3f} (95% {lo:+.3f} to {hi:+.3f})")
    la, lb = leaks(sa), leaks(sb)
    leak_total = sum(e for e, _ in la.values()) + sum(e for e, _ in lb.values())
    print(f"leaks: control {la}, treatment {lb}")
    d, lo, hi = res["Private memory"][:3]
    print("H1 (Private memory >= +0.05 and interval above 0):", "met" if d >= 0.05 and lo > 0 else "not met")
    print("H2 (no leaks in either run):", "holds" if leak_total == 0 else "FAILS")
    harm = [n for n in ("Recall", "Currency") if res[n][0] < -0.02]
    print("H3 (Recall, Currency not more than 0.02 lower):", "holds" if not harm else f"FAILS for {harm}")

if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
