# Actual ARM64 release verification

Source55fc4533ef855d7c0edaebcfe615202663b3ae40 compiled to binary34fbc56050254dd0b3756dca7f630571fe1d70ab195fc24f34083d7620dcaa18/native5b61d3cb. All15 test commands pass:1,175 command-level passes; focused managed4 duplicates full-native4. Two tests are explicitly ignored in their original suites. This is correctness evidence, not profitable-strategy evidence.

Owner independently retrieved the original packet and verified all263 source hashes against Git:61 exact bytes/202 CRLF-only; all actual check log hashes/bytes match. The optimized release completed06:53:17UTC. The initial identity-helper link failed because its native release dependency uses Thin LTO; failed log and checkpoint are retained. A helper-only matching LTO link passed, followed by actual native identity and bridge-helper builds. No source or release rebuild was required.

The original bridge helper pretty-printed a40,065,243-byte15-entry proposal; the unchanged32MiB reader bound rejected it before replay. A separate compact-serialization helper is being qualified; no native-bound expansion or production modification was performed. Current R21 remains executing. Actual candidate prefix parity, controlled deployment and fresh-process trading are separate required observations, not claimed by this packet.

Raw retained account inputs remain private. Source manifest paths and immutable financial-provenance hashes are preserved; original authorization/source/model clocks have not been renewed.
