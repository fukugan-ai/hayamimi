# 1 秒未満の返事の録音 `lat-short`

`lat-short` は、日本語 10 本と英語 10 本の短い返事を、ja と en の交互につないだ 20 発話の録音である。
対面のモードで、1 秒より短い発話の言語判定がどこまで当たるかを記録する IP-7 の入力に使う。
録音と素材は git の外の `testdata/` に置く。この文書は作り方と中身だけを持つ。

| 物 | 置き場 |
|---|---|
| 素材の wav 20 本 | `testdata/lat-short/{ja,en}01.wav` から `10.wav` |
| 素材の manifest（固める前） | `testdata/lat-short/raw.json` |
| 素材の manifest（固めた後） | `testdata/lat-short/pinned.json` |
| つないだ録音 | `testdata/lat-short.wav`（527857 標本） |
| つないだ録音の manifest | `testdata/manifests/lat-short.json` |
| 合成の script | `~/.cache/hayamimi-ips1-20261022/`（`synth.ps1`、`trim.sh`、`words.tsv`、`words2.tsv`。repo に入れない） |

## 声

| 言語 | 声 | 文化 |
|---|---|---|
| ja | Microsoft Haruka Desktop | ja-JP |
| en | Microsoft Zira Desktop | en-US |

Windows の `System.Speech` で合成した。
16 kHz、mono、16 bit の wav を直接書かせている。

## 20 の返事

`ref` は、ja が仮名を含み、en が英字だけで、句読点も数字も付けない。
「clip の標本数」は wav 全体、「speech の長さ」は manifest の `end_sample − start_sample` である。
どちらも 16000 標本（1 秒）未満である。

| clip | lang | 文 | clip の標本数 | speech の長さ |
|---|---|---|---|---|
| ja01 | ja | はい | 12498 | 5120 |
| ja02 | ja | いいえ | 14670 | 7360 |
| ja03 | ja | ええ | 12554 | 5120 |
| ja04 | ja | うん | 12253 | 4800 |
| ja05 | ja | いいね | 14829 | 7360 |
| ja06 | ja | ごめん | 15050 | 7680 |
| ja07 | ja | どうぞ | 15433 | 8000 |
| ja08 | ja | だめ | 13780 | 6400 |
| ja09 | ja | よし | 14714 | 7360 |
| ja10 | ja | ぜひ | 13752 | 6400 |
| en01 | en | No | 13751 | 6400 |
| en02 | en | Yeah | 13686 | 6400 |
| en03 | en | Yep | 15130 | 6720 |
| en04 | en | Nope | 15066 | 6720 |
| en05 | en | Hi | 14358 | 7040 |
| en06 | en | Hey | 14267 | 6720 |
| en07 | en | Bye | 13780 | 6400 |
| en08 | en | Good | 14486 | 7040 |
| en09 | en | True | 15088 | 7680 |
| en10 | en | Cool | 15512 | 8000 |

つないだ録音の manifest では、20 行の `lang` が ja から始まる交互で、切り替えは 19 回である。
間は `--gap-ms 800`（12800 標本）である。

## 作った手順

声の合成は、候補の語を 2 回に分けて作り（ja 30 語、en 32 語）、末尾の無音を切った後に 16000 標本未満に収まった語から、返事らしい語を 10 本ずつ選んだ。
収まらなかった語は、たとえば en の Yes（16703）、Sure（17516）、ja の わかりました（21259）である。

合成は、`words.tsv` と `words2.tsv`（行は「言語、clip 名、文」を tab で区切る）を `synth.ps1` に渡す。

```
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$(wslpath -w synth.ps1)" "$(wslpath -w words.tsv)" "$(wslpath -w candidates)"
```

`System.Speech` は、話し終えた後に約 0.75 秒の無音を wav の末尾に残す。
そのままでは、最初に作った候補 30 語の全部が 16000 標本以上になる（最短の ja04 で 17040）。
末尾の無音を 0.15 秒に切る `trim.sh` を通して、16 kHz、mono、16 bit の wav に書き直した。

```
trim.sh candidates trimmed
```

`trim.sh` の中身は、clip ごとの次の `ffmpeg` である。

```
ffmpeg -i in.wav -af "silenceremove=stop_periods=1:stop_duration=0.3:stop_threshold=-45dB:stop_silence=0.15" -ar 16000 -ac 1 -c:a pcm_s16le out.wav
```

選んだ 20 本を `testdata/lat-short/` に置き、`raw.json`（行は `wav`、`lang`、`ref`）を書いてから、repo の本体を作業 directory にして固め、つないだ。

```
hayamimi-eval fetch-set --pin testdata/lat-short/raw.json --out testdata/lat-short/pinned.json
hayamimi-eval fetch-set --concat --interleave \
  --part manifest:testdata/lat-short/pinned.json:ja \
  --part manifest:testdata/lat-short/pinned.json:en \
  --gap-ms 800 \
  --out testdata/manifests/lat-short.json --wav-out testdata/lat-short.wav
```

## 測れていないこと

声が日本語と英語の別になっているかは、manifest の `ref` の文字までしか機械で測れていない。
合成音声を `hayamimi` に通した認識と言語判定の結果は、この文書の時点では取っていない。
各 clip の後ろの無音は切って 0.15 秒にしたので、つないだ録音の発話と発話のあいだの無音は、clip の末尾の無音と `--gap-ms` の 800 ms を合わせた約 0.95 秒である。
