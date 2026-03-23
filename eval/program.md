# OSM Pixel-Perfect Rendering Evaluation

This is an experiment to have an LLM autonomously improve an OSM tile renderer
toward pixel-perfect matching of the standard OpenStreetMap style.

Inspired by [karpathy/autoresearch](https://github.com/karpathy/autoresearch).

## Setup

To set up a new experiment, work with the user to:

1. **Agree on a run tag**: propose a tag based on today's date (e.g. `mar23`). The branch `autoresearch/<tag>` must not already exist — this is a fresh run.
2. **Create the branch**: `git checkout -b autoresearch/<tag>` from the current branch.
3. **Read the in-scope files**: The repo has these key areas:
   - `CLAUDE.md` — repository context and build instructions.
   - `eval/prepare.py` — fixed data preparation and reference tile fetching. Do not modify.
   - `eval/evaluate.py` — fixed evaluation harness (pixel comparison, scoring). Do not modify.
   - `src/style/` — MapCSS style system (parser, evaluator, types, colors). **You modify this.**
   - `src/renderer/` — Vulkan rendering pipeline. **You modify this.**
   - `shaders/` — GLSL vertex/fragment shaders. **You modify this.**
   - `src/data/` — OSM data loading pipeline. You may modify if needed.
4. **Verify data exists**: Check that `eval/.cache/` contains prepared data and reference tiles. If not, tell the human to run `python eval/prepare.py`.
5. **Initialize results.tsv**: Create `results.tsv` in `eval/` with just the header row. The baseline will be recorded after the first run.
6. **Confirm and go**: Confirm setup looks good.

Once you get confirmation, kick off the experimentation.

## Experimentation

Each experiment modifies the renderer to better match the standard OSM tile style. You evaluate by running `python eval/evaluate.py` which:

1. Builds the project (`cargo build --release`)
2. Starts the tile server and measures startup time
3. Renders tiles at zoom levels 10–16 within Hamburg
4. Compares each rendered tile pixel-by-pixel against OSM reference tiles
5. Computes per-zoom and aggregate scores

**What you CAN do:**
- Modify Rust source code in `src/` — style evaluation, renderer, shaders, data pipeline.
- Modify GLSL shaders in `shaders/`.
- Add or modify MapCSS style rules.
- Adjust rendering parameters (line widths, colors, polygon fills, etc.).
- Add new rendering features (text labels, area patterns, etc.).

**What you CANNOT do:**
- Modify `eval/prepare.py` or `eval/evaluate.py`. They are read-only.
- Change the evaluation metric or scoring functions.
- Install new system-level dependencies beyond what's available.
- Modify reference tiles.

**The goal is simple: maximize the combined_score.** This score is the minimum of three sub-scores (visual fidelity, render performance, startup time), each normalized to 0–1. The worst dimension always dominates, ensuring you must balance all three goals.

**Performance constraint**: All tiles MUST render in <50ms. The render_score penalizes any zoom level where the worst tile exceeds this target.

**Simplicity criterion**: All else being equal, simpler is better. A small improvement that adds ugly complexity is not worth it. Conversely, removing something and getting equal or better results is a great outcome.

**The first run**: Your very first run should always be to establish the baseline, so run the evaluation as-is.

## Scoring System

The evaluation produces three normalized scores (0.0 to 1.0):

1. **visual_score**: Pixel-perfect match ratio against OSM reference tiles. Minimum across all zoom levels (worst zoom dominates). A score of 1.0 means every pixel matches the reference exactly.

2. **render_score**: Rendering speed. For each zoom level, score = min(1.0, 50ms / max_render_time). Minimum across zoom levels. A score of 1.0 means all tiles render in ≤50ms.

3. **startup_score**: Server startup time. score = min(1.0, 30s / startup_time). A score of 1.0 means the server starts in ≤30s.

4. **combined_score**: min(visual_score, render_score, startup_score). This is the single number to optimize.

The min() aggregation means you can only improve the combined score by improving whichever dimension is currently worst.

## Output format

The evaluation script prints a summary like this:

```
============================================================
EVALUATION RESULTS
============================================================

Build time:   12.3s
Startup time: 8.5s

Per-zoom results:
  Zoom Tiles    Match   Avg ms   Max ms  V-Score  R-Score
  ---- -----  --------  ------  -------  -------  -------
    10     4    0.0523    12.3     15.1   0.0523   1.0000
    12    42    0.0312     8.7     22.4   0.0312   1.0000
    14   100    0.0198    11.2     35.8   0.0198   1.0000
    16   100    0.0089    15.4     48.2   0.0089   1.0000

Aggregate Scores:
  Visual (pixel match):  0.0089
  Render (time target):  1.0000
  Startup:               1.0000

  COMBINED SCORE:        0.0089
```

You can extract the key metric:
```
python eval/evaluate.py 2>&1 | grep "COMBINED SCORE"
```

## Logging results

When an experiment is done, log it to `eval/results.tsv` (tab-separated).

The TSV has a header row and 5 columns:

```
commit	combined_score	visual_score	status	description
```

1. git commit hash (short, 7 chars)
2. combined_score achieved (e.g. 0.0523) — use 0.000000 for crashes
3. visual_score achieved (e.g. 0.0523) — use 0.000000 for crashes
4. status: `keep`, `discard`, or `crash`
5. short text description of what this experiment tried

Example:

```
commit	combined_score	visual_score	status	description
a1b2c3d	0.0089	0.0089	keep	baseline
b2c3d4e	0.0234	0.0234	keep	add road color matching for motorways
c3d4e5f	0.0198	0.0198	discard	experimental line width scaling
d4e5f6g	0.000000	0.000000	crash	shader compilation error
```

## The experiment loop

The experiment runs on a dedicated branch (e.g. `autoresearch/mar23`).

LOOP FOREVER:

1. Look at the git state: the current branch/commit we're on
2. Decide on a rendering improvement to try (better colors, line widths, area fills, labels, etc.)
3. Modify the relevant source files (Rust code, shaders, style rules)
4. git commit
5. Run the evaluation: `python eval/evaluate.py > eval/run.log 2>&1`
6. Read out the results: `grep "COMBINED SCORE\|visual_score\|render_score\|startup_score" eval/run.log`
7. If the grep output is empty, the run crashed. Run `tail -n 50 eval/run.log` to diagnose.
8. Record the results in the tsv (NOTE: do not commit results.tsv, leave it untracked)
9. If combined_score improved (higher), you "advance" the branch, keeping the commit
10. If combined_score is equal or worse, git reset back to where you started

**Hints for improving visual_score:**
- Study the OSM standard style (Mapnik/CartoCSS). The reference tiles use the standard openstreetmap-carto style.
- Key elements to match: road colors by type (motorway=blue, primary=orange, etc.), road widths by zoom, landuse area fills, water bodies, building outlines.
- Compare rendered vs reference tiles visually (they're saved in `eval/.cache/rendered_tiles/` and `eval/.cache/reference_tiles/`).
- Start with the biggest visual differences first — matching background/landuse colors gives the most pixel improvement.

**Timeout**: Each evaluation should take a few minutes. If it exceeds 15 minutes, kill it and treat as failure.

**Crashes**: If the build fails or server crashes, fix the issue and re-run. If the idea is fundamentally broken, skip it and move on.

**NEVER STOP**: Once the experiment loop has begun, do NOT pause to ask the human if you should continue. The human might be asleep. You are autonomous. If you run out of ideas, think harder — study the reference tiles, read the OSM wiki for style specifications, try combining previous near-misses. The loop runs until the human interrupts you.
