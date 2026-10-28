# ACT-R literature fixtures for `src/actr.rs`

Verified against primary sources (ACT-R 7.x Reference Manual, Dan Bothell,
CMU; ACT-R tutorials Unit 4/Unit 5) via the Wayback Machine, since
`act-r.psy.cmu.edu` is unreachable from this environment. Use these as
hard-coded fixtures in the ACT-R reference module's unit tests — do not
invent additional "from the literature" numbers.

These fixtures only cover `fan_strength` (Fixtures 1 and 2 below) - the one
function in `src/actr.rs` with a published worked example behind it. No
primary source checked gives a worked numeric example for
`base_level_activation`, `retrieval_probability`, or `sample_noise`; their
tests in `src/actr.rs` instead check closed-form identities (e.g. `P = 0.5`
at `τ = A_i`) and statistical properties (noise mean/variance over many
draws), not literature fixtures. See Gaps below for what's missing and why.

## Equations (Reference Manual, p.292–296)

- Standard: `B_i = ln(Σ_{j=1}^n t_j^-d) + β_i`
- Optimized-learning approximation: `B_i = ln(n/(1-d)) - d*ln(L) + β_i`
- Fan effect: `S_ji = S - ln(fan_ji)`, `fan_ji = (1 + slots_j) / slotsof_ji`
- Retrieval time: `RT = F * e^(-f*A_i)`; failure time: `RT = F * e^(-f*τ)`
- Noise: logistic, `σ² = (π²/3) s²`
- Retrieval probability (Tutorial Unit 4, p.5): `P_i = 1 / (1 + e^((τ - A_i)/s))`

## Parameter defaults (Reference Manual)

| Param | Meaning | Default | Recommended |
|---|---|---|---|
| `:bll` (`d`) | decay | `nil` (off) | **0.5** — "one of the few parameters which have a strong recommended value" (p.299) |
| `:rt` (`τ`) | retrieval threshold | **0.0** (p.304) | — |
| `:ans` (`s`) | noise scale | `nil` (off) | **[0.2, 0.8]** (p.299); teaching chapters cite ~0.25 as typical (secondary) |
| `:mas` (`S`) | max associative strength | `nil` (off, spreading activation) | no single point value given |

## Fixture 1 — Reference Manual `sdp` worked example (p.319-320)

`(sgp :esc t :bll .5 :mas 2 :mp 1)`, chunks A, B, C, D; B and C both have `slot1 a`.

- `S(A,A) = 2 - ln(3) = 0.9013877113318902` (fan 3) — manual prints `0.9013877`
- `S(B,·) = S(C,·) = 2 - ln(2) = 1.3068528194400546` (fan 2) — manual prints `1.3068528`
- Base-level at n=1 presentation: `B = 2.191` (elapsed time `t` not given in the excerpt — do not back-derive it)
- `(sdp a :permanent-noise .3)` → `Activation 2.491 = 2.191 + 0.300` (confirms noise adds linearly)

## Fixture 2 — Tutorial Unit 5 fan-effect examples (8-Feb-05)

Model: `F = .63`, `S (:mas) = 1.6`, `B_i = 0` for all chunks.

**"The lawyer is in the store"** (fan(store) = fan(lawyer) = 2):
- `S_ji = 1.6 - ln(2) = 0.9068528194400548` (trace prints `0.907`)
- `A = 0.5*0.907 + 0.5*0.907 = 0.907`
- `Time = .63 * e^(-0.907) = 0.254s`; total RT = **1.084s**

**"The hippie is in the bank"** (fan(hippie) = 4; the tutorial's prose
labels fan(bank) = 2, but its own printed trace is only consistent with
fan(bank) = 3 - see Gaps below; the implementation and its tests use the
verified trace numbers, not the prose label):
- `S(hippie) = 1.6 - ln(4) = 0.21370563888010952`
- `S(bank) = 1.6 - ln(3) = 0.5013877113318902` (matches the printed `0.501`; `1.6 - ln(2) = 0.907` does not)
- `A = 0.5*0.214 + 0.5*0.501 = 0.358`
- `Time = .63 * e^(-0.358) = 0.441s`

## Gaps (explicitly not filled with invented numbers)

- **fan(bank) discrepancy, found implementing the fixture.** The tutorial
  excerpt's own prose says fan(bank) = 2, but its printed trace values
  (S = 0.501, A = 0.358, RT = 0.441s) only reproduce from `1.6 - ln(3)`,
  not `1.6 - ln(2) = 0.907`. Either the prose mislabels the fan count, or
  a detail of the example (a second sentence referencing "bank", a extra
  slot) isn't visible in the excerpt. The code and its tests trust the
  verified printed trace over the prose label. Re-check against the full
  tutorial PDF before citing fan(bank) = 2 or = 3 anywhere outside this
  codebase.
- No source gives raw presentation times `t_j` → a stated numeric `B_i`
  (only n, d, and the result, in Fixture 1).
- No source gives a numeric `(A, τ, s) → P` worked example — only the
  qualitative fact that `P = 0.5` when `τ = A_i`.
- Pavlik & Anderson 2005 uses adaptive decay, not fixed `d = 0.5`, so it
  cannot supply a `d = 0.5` example either.

Source agent's verification: primary for all of the above except the
`s ≈ 0.25` note (Taatgen, Lebiere & Anderson, "Modeling paradigms in
ACT-R" — secondary, a teaching chapter by ACT-R core developers, not the
manual itself).
