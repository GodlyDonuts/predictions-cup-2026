# Retain the stronger measured founder

The complete, matched CPU comparison of population037's 540-update checkpoint against its original founder found regression, not improvement. Both used the full October17–19 development interval, the original 84ms actor cost and the same nominal/stressed case seeds. No optimizer or independent test was used.

| Policy | Nominal marked ROI | Stressed marked ROI | Worst daily-growth reward |
|---|---:|---:|---:|
| Original founder | -4.0442175% | -2.693465% | -0.060336735 |
| 540-update checkpoint | -4.2286550% | -17.0649575% | -0.242723737 |

The founder is the stronger measured policy. Neither qualifies as profitable. These returns use causal SIG midpoint valuation and uncalibrated passive execution; they do not establish executable liquidation or production returns. The trained checkpoint spent essentially all100,000 simulated starting cash, and all fills were purchases. Tiny paper allocation is not the cause of its losses. [Exact results, hashes and attribution](population-037-founder-comparison-20261021.json).

The existing population loop trained every initial founder before evaluating it. Champion retention worked between later generations, including negative rewards, but did not protect a stronger initial founder against regression during that first training pass. The explicit `evaluate_founders_before_training: true` option fixes this: evaluate each founder on the same evolution/development cases first, seed the island champions, then use the existing retention/replacement logic. Scores remain inside their original experiment. All checkpoint files are retained. No CPU score is imported into the active GPU run.

A positive founder selected by a shortened development screen receives the same full-window confirmation as a later positive screen winner. Founder evaluation cannot consume an independent test. Twenty-one focused founder/population/asynchronous-island checks pass, including a complete real initialization, checkpoint, training and replacement lifecycle with controlled negative fitness. The regression check proves actual optimizer updates occur and the stronger founder survives a worse first trained generation.

Active population037 remains frozen. Both islands have now saved their first trained member checkpoint and are evaluating; the first finished all eight full training worlds after2,773 updates. A separate frozen2,768-update checkpoint is still undergoing full-window CPU evaluation. Its score must come from that completed evaluation, not from its update count. The new founder option and64-transition batch target are intended for the next admitted configuration, not silently applied to active037. Protected October21–25 is unconsumed; no learned policy is deployed.
