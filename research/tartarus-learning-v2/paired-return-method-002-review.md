# Streaming versus closed-episode return targets

The predeclared CPU comparison completed both arms using the same four fresh H128 founders, identical initial parameter hashes, shared4ms decision-cost calibration, four simulated worlds,60-second training episodes and two60-second nominal/stressed development-selection blocks. It uses only the previously inspected short three-venue corpus. This is not a four-day independent test, full58.2M-model GPU qualification or profitable checkpoint.

| Method | Wall seconds | Training decisions | Accepted updates | Four candidates' worst marked development ROI |
| --- | ---: | ---: | ---: | --- |
| Streaming bootstrapped returns | 1842.023 | 62,434 | 1,467 | -15.593%, -18.745%, -20.524%, -19.522% |
| Sealed closed-episode returns | 2641.887 | 68,884 | 1,371 | -14.045%, -26.163%, -10.718%, -20.442% |

Closed-episode returns improved two paired candidates and worsened two. Every candidate remained negative. Closed-episode wall time was about1.43times streaming in this particular CPU run; the policies' later actions and collected-decision counts diverged as expected after learning, so this is not an isolated optimizer-kernel speed ratio. Four matched founders provide insufficient evidence of a general method winner. The active GPU learner keeps streaming returns; no automatic method switch or production deployment follows from this result.

The complete candidate losses, founder hashes, experiment identities and paired differences are retained in [the comparison artifact](paired-return-method-002-result.json). Source was frozen at `d8fbf96a8db3e91c2e938cbf36ff6df18f9c40a7`, including the joint-memory probability correction. No independent test was opened and no production financial state changed.
