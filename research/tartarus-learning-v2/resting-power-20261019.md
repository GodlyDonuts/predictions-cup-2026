# Resting buy funding under counterfactual fills

The optional `resting_buy_funding` execution profile resizes or cancels unaffordable resting BUY remainders after fills change cash and available advance. It also sizes a newly resting remainder against then-current buying power, ignores expired buys when calculating same-exchange commitments, and reports reduced placed quantity in scoped HTTP acknowledgments. Resting SELL inventory and settlement/eligibility transitions are separate unfinished work. No new artificial decision, portfolio, quantity or cross-exchange escrow cap is introduced.

The cached official v1 OpenAPI SHA256 `fbdf23e54d317392f9fb4a659b9511a01fad065e7a384f28091ef04894a732c9` describes this in `POST /orders` and `Order.quantity`: resting cash is not escrowed; same-exchange unexpired buys affect admission; buys on other exchanges do not count against that exchange's commitments; and declining buying power can shrink or cancel existing buys. The schema does not specify survival priority among multiple same-exchange orders. This source is saved documentation, not a fresh live-engine calibration.

Two explicit hypotheses are available:

- `per_exchange_oldest_first_uncalibrated`: each exchange independently receives current account buying power, with older placed orders/IDs using that exchange's allowance first.
- `per_order_independent_uncalibrated`: each individual remainder is bounded by current buying power; aggregate same-exchange survival priority is not imposed. Initial same-exchange admission still follows the documented commitment rule.

Neither mode represents calibrated authoritative priority. A serious strategy result needs sensitivity testing across plausible rules and eventual source calibration. Omitting the flag retains earlier replay behavior; frozen GPU017 and queued018 have not been modified. The mode is pinned in the experiment configuration and source identity; no tensor architecture change is required. Development founder transfers start with empty optimizers and fresh score evidence.

Component-funded cash/debt finalizes before resizing. Exact integer micro-unit costs, original fill quantities and FIFO economics remain separate from discarded quote size. A resized order never grows back automatically when funds recover. Order history records the decrease, and atomic basket rollback restores the prior size/history/debt along with finance and depth. Matching checks the live remainder between previewed price levels so a funding-driven cancellation cannot leave a stale executable preview.

Resizing is hidden physical state until a scoped paid order read reconciles it; no free cash/positions/advance baseline is exposed. Private fill notifications retain their prior behavior. **Dedicated private resize pushes are not modeled**, so this remains an incomplete account-topic projection pending calibration. No unsupported terminal reason code is invented for funding cancellations. Closed historical order quantities use the actual reduced remainder plus fills, rather than restoring discarded submitted size. Owned-order-page receipts are now included in the observable wake index as well.

## Checked evidence

The full learning-actions suite passed313 tests in74.275seconds. The final10-case focused suite additionally passed after adding a stale multi-price preview/funding-shock case. Focused coverage includes cross-exchange non-escrow, same-exchange admission/no-op, expired commitments, paid-only order knowledge, zero-power cancellation and no resurrection, reduced reported quantities, both priority hypotheses, component-debt finalization, exact atomic rollback and legacy-profile preservation. The funding shock is synthetic and is not presented as an observed historical account event.

The actual recorded three-venue factory ran two H32 founders/two worlds/two short evolution cycles:927 decisions and35 accepted updates. Every founder tensor matched its bulk-quote parent and optimizer states began empty; all runtime source pins were rechecked. Four candidate minimum development ROIs were0%, over seconds of selection history, not complete daily reward cases. [Exact source-bound evidence](resting-power-recorded-001.json).

Separate recorded-book physical probes exercised both hypotheses: a20000-share passive paper quote became19904 shares after a different exchange's one-share counterfactual fill cost480000 micro-units. Each paid order read then matched the physical remainder, without free cash/position/order publication. These probes use genuine original book depth but deliberately chosen paper actions and a fresh100-SUSQie paper account; they are execution integration checks, not learned profitable trades or production transactions. Linked-component tests remain synthetic because verified causal historical topology is unavailable.

No account client, production control, STOP, activation, bank or financial state was changed. Full chronological coverage and independently positive ROI remain unproven; see the [data/exposure audit](history-readiness-20261019.md).
