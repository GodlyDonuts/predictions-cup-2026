# Missing contrary primary evidence in Maine

Viserion/20261012-maine-unh investigated the outstanding UNH source and found another omitted primary poll. The four-poll source set has Democratic margins +4, +2, 0 and +4. [Siena's September 24 release](https://sri.siena.edu/2026/09/24/nytimes-siena-poll-of-likely-voters/) reports Susan Collins 49% and Troy Jackson 46%, a Democratic margin of **-3 points**. This source was not in that manifest.

The poll's [co-publisher account](https://www.pressherald.com/2026/09/24/5-takeaways-from-the-new-nyt-press-herald-siena-poll/) gives September 15-22 fieldwork, 619 registered voters overall, and reported sampling error of 4.9 points. Siena labels its release likely voters. The linked [detailed toplines](https://www.nytimes.com/interactive/2026/09/24/polls/times-siena-poll-me-toplines.html) could not be opened. The question-specific sample/population therefore remains unresolved. These voter percentages and sampling errors are not probabilities of an election win.

The [UNH publication record](https://scholars.unh.edu/survey_center_polls/1007/) verifies the September 24 release and qualitatively describes Jackson and Collins as statistically tied. Its [PDF](https://scholars.unh.edu/cgi/viewcontent.cgi?article=2006&context=survey_center_polls) still returns 403. No third-party numerical toplines, sample or guessed fieldwork date were added. UNH stays out of the numerical sensitivity.

## Unchanged-model sensitivity

At the original reference clock, keep each existing historical error model, the declared recency family, equal per-poll treatment, and 0.02 value discount unchanged. Add only Siena's documented -3 margin and co-publisher field-end date. This is a conditional sensitivity, not a repaired production bank or validated transport model.

| Recency half-life | Original reference | With Siena | Historical quote proposal |
|---|---:|---:|---|
| 7 days | 0.624568 | 0.600081 | 824 shares instead of 1,724 |
| **14 days, declared default** | **0.625267** | **0.581087** | **0 shares: reference gap below 0.005** |
| 28 days | 0.625191 | 0.567142 | 0 shares: reference below quote |

The old NO quote is 0.580, from the dated book. Under the declared default, the residual reference gap is about 0.001087, below its existing minimum. The 7-day family is not selected because it still accepts a quote. No family/model was refitted or chosen by this result. The 14-day minus-three-point sensitivity falls to about 0.368333; it is an assumption, not a probability bound.

Run `node research/viserion-maine-unh-20261012/sensitivity.mjs`. It requires exact reproduction of all three original weighted margins and reference values before producing results, then applies the existing passive quote/sizing module. `sources.json` records source clocks, HTTP results and body hashes without storing full publisher text. `results.json` records input hashes, unchanged model parameters and the dated quote outcomes.

Fresh source retrieval does not refresh poll fieldwork or the old market book. Mixed populations, incomplete census, unvalidated population transport and the gap between election probabilities and short-term harvest prices remain. No current executable depth, actual fills, market profit, bank rewrite, collector or activation is claimed. Next: obtain Siena's detailed population/sample and the UNH-authored report, complete the source census, and run prospective funded execution evidence before scaling.
