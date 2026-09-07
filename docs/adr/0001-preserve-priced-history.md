# Preserve priced history while allowing explicit initial backfill

Estimated token cost values local usage at user-configured prices, so applying every price edit retroactively would change historical valuations. A model's first configured price may explicitly cover its older unpriced usage, including when that model is configured later; subsequent edits create price versions effective for future usage, while already priced history remains unchanged across restarts and reimports. This trades automatic historical recalculation for stable historical values while allowing the user to fill an initial pricing gap.

## Consequences

- Pricing history must retain enough information to preserve the valuation of already priced usage across restarts and reimports.
- Unpriced usage remains unknown rather than zero: display the known cost subtotal as incomplete when relevant usage is unpriced, and suppress the observed cost per weekly percentage point for that scope.
