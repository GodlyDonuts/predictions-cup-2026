# Frozen inherited profit depth census

Reproduce privately: `python frozen-depth-census.py` with the original private `actual-exit-audit-v4` input directory beside the script. Inputs are untouched files extracted from original ZIP SHA11167216c88850ff5414f16415f57e00e4924e255dce2174518c1c9f2b40432f. Script verifies raw pins, original native suffix row hashes/chain and exact tail against sealed report, plus original dispatch row hash chain.

As of native sequence452287 /380fills /2026-10-17T08:34:33.355Z,14 unmanaged inherited unclocked held sides remain after excluding11 native managed IDs, clocked/mixed lots and protected packages (sealed report has no livePackages: native package map empty). Reconstruct latest valid books in original sequence; every gap or invalid book withdraws preceding book, a subsequent original valid book restores native validity. This does not independently authenticate underlying REST bodies or prove a complete trade tape.

Only948NO passes .005/share worst-lot gain at the frozen clock. Held3954; book source08:34:29.9299526, original request08:34:29.767, receipt08:34:30.050. Best NO bid .900 from YES ask .100,110shares at qualifying prices. Native50% slice yields55shares, conservative limit .900, hypothetical limit proceeds49.500, FIFO basis49.225, marked gain.275. Existing unused native SELL14719 reserves69shares; this is a NON-ADDITIVE alternative depth diagnostic, not an additional executable order. No original dispatch intent/attempt/ACK for14719 in the captured full chain.

Eight other sides have stale books; five have fresh books but insufficient marginal gain. All13 fail gain even ignoring staleness, so this snapshot supplies no second positive inherited nomination. Missing/empty/final-invalid counts are zero within the captured suffix, not a claim about markets outside this scope.

Relevant actual policy fields were independently confirmed by root from pinned R22Control SHA81ddfa2d30bc4385e709dbeea3af0250cb8a808df8c60fe89cb13acfc9595b51: zero fees, minimumGain.005, bookAge10000ms, maxquantity10000, depth5000bps. Full normalized policy is not present; full policyHash parity was not recomputed. Actual profitableDepthMode record is enabled with5000bps and policyHash7025870102e5e2e3449fc0381cc22558a7731c50a9837eac2c59782e9a96a74d.

All data are historical and asynchronous. Fresh original account/open orders/contract/source continuity/queue/physical authority remain mandatory. No simulated fills, added profit across observations, missed-fill assertion, reservation release, retirement, or trading permission.
