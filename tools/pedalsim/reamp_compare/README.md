# Re-amp comparison

Compares recordings of the same DI played through the pedal with different uploads against a reference (the
original NAM model and IR rendered in software on the pedal's own dry input), and builds the interactive page
the docs site shows (`site/src/analysis/session2.html`).

```sh
# 1. analyse: align, loudness-match, spectra, moment-to-moment error, heatmap, dynamics -> data.json, plus
#    loudness-matched WAVs of every take in <recordings dir>/compare/
python analyze.py <recordings dir> data.json '[["ref","ref.wav","Reference"],["new","take1.wav","…"],…]' dry.wav
# 2. the page (add --audio BASE EXT NOTE to include the A/B player)
python build.py data.json page.html --back comparison.html "Back to the docs"
# 3. the static graph for the docs
python plots.py data.json ../../../docs/images/nam-reamp-difference.png
```

The first take is the reference; the keys `new`, `ori`, `old` and `vendor` pick the colours. `session2.json` is the
data of the 2026-10-22 session (Ampeg SVT-VR Classic (Normal) + Ampeg SVT Beta52); its titles and notes were edited
for the site after analysis. The recordings themselves aren't in the repository.
