# Checked entry-capital release candidate

Source [8251b883](https://github.com/GodlyDonuts/predictions-cup-2026/commit/8251b883d07f2dffa54c55f8c0f7dea20809ce05), ARM64 binary `0bc69f072874263fdf295556d0e185a98041ca4060cfeb731c2e34f6d43b8d41`. Actual worker136 and selector13 checks pass, followed by the optimized locked/offline production build. [Qualification](qualification.json), [independent root comparison](root-verification.json).

All263 actual build inputs match the published commit:59 exact bytes and204 equivalent after CRLF normalization. The financial engine fingerprint uses the exact original16 raw inputs and remains `5b61d3cbb38eadf5170fafae5c91cb5b75858346b2771031084224bfa2c3d6d4`. Native, SDK, model, source and risk/rate/activation settings are unchanged.

The first selector run passed12/13. Its new same-clock fixture never generated the second market's quotes under real native pacing. Only that fixture's second book clock changed to2500ms; the affected13 reran successfully, with worker inputs unchanged. Failed original log/result/manifest are retained privately. The public qualification does not conceal that correction.

The independently authenticated private packet contains270 original files,7,264,499bytes, SHA256 `821527314d45c0bc22d26a274cf0d650ed4f50b7997a7c33f58a2c45100e8a97`. Root checked every member, source packet, test result, binary and source file against Git. This qualifies the candidate; it does not establish installation, live reservation release or profit. Existing R24 remained live during compilation.
