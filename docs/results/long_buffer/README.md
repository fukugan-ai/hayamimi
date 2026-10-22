# Long buffers read by the Japanese tier in one call

RP-89 asks whether the Japanese tier (`Tier::Rz`), given a buffer of 20 s or more in one uncapped call, reads fewer of the sentences than when the same sentences are given one clip at a time. The test is `native/hayamimi-asr/tests/rp89.rs`. This directory holds the numbers of the first run and the checksums of the audio it read. The audio is not in git (`testdata/` is ignored).

    cd native && cargo test --release -p hayamimi-asr --test rp89 -j 2 -- --ignored --nocapture

The run in this file was typed through `cargo-slot` with `nice -n 10`. The acceptance command in `docs/design/rust_port.ja.md` (RP-89) uses `sudo -n nice -n -5`; an error rate does not depend on the priority.

## Material

Three groups of three clips of `testdata/eval_real`, in this order, joined with 0.2 s and with 0.5 s of zero samples between the clips and none before the first or after the last. `ja_06` is not used. The reference of a group is its three references written one after another with nothing between them.

| group | clips | reference characters |
|---|---|---|
| L1 | `ja_10`, `ja_13`, `ja_15` | 56 |
| L2 | `ja_05`, `ja_08`, `ja_07` | 65 |
| L3 | `ja_03`, `ja_09`, `ja_04` | 62 |

The six files are in `testdata/long_ja/` (16 kHz, mono, 16 bit). They were made from the repository root with `hayamimi-eval fetch-set`, once per group: a three-row manifest `<group>.src.json` (`wav` is `../eval_real/ja_NN.wav`) is pinned, then joined with `--gap-ms` set explicitly (its default is 1500).

    hayamimi-eval fetch-set --pin testdata/long_ja/L1.src.json --out testdata/long_ja/L1.pinned.json
    hayamimi-eval fetch-set --concat --part manifest:testdata/long_ja/L1.pinned.json:ja --gap-ms 200 --wav-out testdata/long_ja/L1_g200.wav --out testdata/long_ja/L1_g200.json

| file | samples | seconds | sha256 |
|---|---|---|---|
| `L1_g200.wav` | 323218 | 20.201 | `b237fbb65acdca773504e54e0f1cab382bed44ec3b4c2fbc3bc70bb28bde306d` |
| `L1_g500.wav` | 332818 | 20.801 | `e867518432d83cbb993ae228cc1e373b2f46c2a83fbea324d6e4351b2438f6b8` |
| `L2_g200.wav` | 321116 | 20.070 | `a15f612ad9f570a1b8091b87a74dee9b5a3a81ba2264fee7b246d52acaf7c7f7` |
| `L2_g500.wav` | 330716 | 20.670 | `9de79677b856fe1e7922e67fd1926f434afc1b1002d5f85bb5c6485af2fae014` |
| `L3_g200.wav` | 329519 | 20.595 | `53548644dc25d58af8621e5c876dddd58ffc9c7272970e3c0e34445784e77dec` |
| `L3_g500.wav` | 339119 | 21.195 | `7e121700c76f4b6778a285ceb078508c5ed58253cf4ed0298b92822af4434e95` |

The test `the_long_material_is_the_three_clips_joined_by_zero_gaps` reads the six files back and checks each sample count, that every gap sample is zero, and that every clip sample equals its source.

## Method

Every number is a character error rate from `hayamimi_eval::cer::cer` (NFKC, punctuation and spaces removed, edit distance over code points), in millionths. The decoder is `Recognizers::decode_whole(Tier::Rz, ..)`, one call per buffer with no cap and no split retry.

- Whole: the joined buffer in one call, against the reference of the group.
- One by one: the three clips of the group in three calls, each against its own reference, distance and length summed over the three. It does not depend on the gap, so each group's value appears under both gaps and the total counts it twice.
- Total: distance and length are added over the six buffers and then divided (no mean of rates).

The line of RP-89 is a whole-decode total at least 50000 millionths (5.0 pt) above the one-by-one total.

## Result

| buffer | whole | one by one | difference | whole characters / reference |
|---|---|---|---|---|
| L1, 0.2 s | 285714 (16/56) | 107142 (6/56) | 178572 | 42 / 56 |
| L1, 0.5 s | 321428 (18/56) | 107142 (6/56) | 214286 | 40 / 56 |
| L2, 0.2 s | 553846 (36/65) | 76923 (5/65) | 476923 | 39 / 65 |
| L2, 0.5 s | 661538 (43/65) | 76923 (5/65) | 584615 | 32 / 65 |
| L3, 0.2 s | 258064 (16/62) | 80645 (5/62) | 177419 | 66 / 62 |
| L3, 0.5 s | 193548 (12/62) | 80645 (5/62) | 112903 | 62 / 62 |
| total | 385245 (141/366) | 87431 (32/366) | 297814 | |

The total difference is 29.8 pt against a line of 5.0 pt, and each of the six buffers is above 11 pt, so RP-89 holds. The character counts show two shapes: L1 and L2 return 49 to 75 per cent of the reference characters (characters are missing), while L3 returns about as many characters as the reference and still makes 12 to 16 errors against 5 for the clips one by one (characters are wrong). Why the Japanese tier fails on a long buffer is not established by this run.

Controls, same run:

| case | whole | one by one | difference |
|---|---|---|---|
| `a_single_clip_group_shows_no_drop`, the nine clips alone (4.8 s to 8.8 s) | equal | equal | 0 for all nine |
| `the_existing_51_second_buffer_shows_the_same_drop`, `ja_01` to `ja_08` with 0.5 s after each (827938 samples) | 708074 | 62111 | 645963 |

Per clip, whole and one by one are the same decode of the same samples: `ja_10` 83333, `ja_13` 230769, `ja_15` 52631, `ja_05` 217391, `ja_08` 0, `ja_07` 0, `ja_03` 0, `ja_09` 0, `ja_04` 384615 millionths.

## Measured with

- Base: `bdc071a` of `design/long-buffer`, `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `cargo test --release`.
- The four ignored tests of `rp89.rs` ran in one process on parallel test threads. Whether the decoder ran on the GPU or the CPU was left to `Gpu::Try` and is not recorded.
- These numbers come from one run; a second run was not made.
