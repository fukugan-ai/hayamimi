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
末尾の無音を切る `trim.sh` を通して、16 kHz、mono、16 bit の wav に書き直した。
`silenceremove` は `stop_duration`（0.3 秒）と `stop_silence`（0.15 秒）の合計が末尾に残るので、切った後の末尾の無音は 20 本で 0.428〜0.464 秒である（`silencedetect` の閾値 -45 dB、最後の無音の始まりから wav の終わりまで）。

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
つないだ録音の発話と発話のあいだ（次の `start_sample` − 前の `end_sample`）は、19 か所とも 20086〜21210 標本（1.255〜1.326 秒）である。
speech の終わりの検出が clip の終わりより前に来る分と、clip の末尾の無音と、`--gap-ms` の 800 ms を合わせた値になる。

## 再現に使った script

repo には script の file を足さない。次の 4 つが `~/.cache/hayamimi-ips1-20261022/` の内容である。

`synth.ps1`：

```powershell
# Makes one 16 kHz mono 16-bit wav per line of the words file, with the
# desktop voice of the line's language (System.Speech).
# usage: powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File synth.ps1 <words.tsv> <out dir>
param([string]$Words, [string]$OutDir)
Add-Type -AssemblyName System.Speech
$voices = @{ 'ja' = 'Microsoft Haruka Desktop'; 'en' = 'Microsoft Zira Desktop' }
$format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(
    16000,
    [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono)
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
foreach ($line in Get-Content -Encoding UTF8 -Path $Words) {
    if ($line.Trim().Length -eq 0) { continue }
    $cells = $line.Split("`t")
    $lang = $cells[0]; $stem = $cells[1]; $text = $cells[2]
    $synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
    $synth.SelectVoice($voices[$lang])
    $synth.SetOutputToWaveFile((Join-Path $OutDir ($stem + '.wav')), $format)
    $synth.Speak($text)
    $synth.Dispose()
    Write-Output ($stem + ' ' + $voices[$lang])
}
```

`trim.sh`：

```bash
#!/usr/bin/env bash
# Cuts the silence that System.Speech leaves after a reply in a wav file
# (about 0.75 s) down to about 0.45 s (stop_duration 0.3 s plus stop_silence
# 0.15 s both remain), and writes 16 kHz mono 16-bit wavs.
# usage: trim.sh <in dir> <out dir>
set -eu
in_dir=$1
out_dir=$2
mkdir -p "$out_dir"
for wav in "$in_dir"/*.wav; do
    name=$(basename "$wav")
    ffmpeg -hide_banner -loglevel error -y -i "$wav" \
        -af "silenceremove=stop_periods=1:stop_duration=0.3:stop_threshold=-45dB:stop_silence=0.15" \
        -ar 16000 -ac 1 -c:a pcm_s16le "$out_dir/$name"
    samples=$(ffprobe -v error -select_streams a:0 -show_entries stream=duration_ts -of csv=p=0 "$out_dir/$name")
    echo "$name $samples"
done
```

`words.tsv`（行は言語、clip 名、文を tab で区切る）：

```
ja	ja01	はい
ja	ja02	いいえ
ja	ja03	ええ
ja	ja04	うん
ja	ja05	そうです
ja	ja06	なるほど
ja	ja07	どうも
ja	ja08	了解です
ja	ja09	結構です
ja	ja10	いいですよ
ja	ja11	もちろん
ja	ja12	確かに
ja	ja13	ありがとう
ja	ja14	すみません
ja	ja15	わかりました
ja	ja16	お願いします
en	en01	Yes
en	en02	No
en	en03	Okay
en	en04	Sure
en	en05	Thanks
en	en06	Right
en	en07	Exactly
en	en08	Great
en	en09	Agreed
en	en10	Fine
en	en11	Understood
en	en12	Correct
en	en13	Absolutely
en	en14	Perfect
```

`words2.tsv`：

```
ja	ja21	ああ
ja	ja22	えっ
ja	ja23	ほう
ja	ja24	まあ
ja	ja25	へえ
ja	ja26	あっ
ja	ja27	よし
ja	ja28	ごめん
ja	ja29	どうぞ
ja	ja30	いいね
ja	ja31	うそ
ja	ja32	ふむ
ja	ja33	だめ
ja	ja34	ぜひ
en	en21	Yeah
en	en22	Yep
en	en23	Nope
en	en24	Hi
en	en25	Hey
en	en26	Bye
en	en27	Well
en	en28	Wow
en	en29	Cool
en	en30	Good
en	en31	True
en	en32	Done
en	en33	Nice
en	en34	Hmm
en	en35	Oh
en	en36	Please
en	en37	Sorry
en	en38	Maybe
```

選んだ 20 本は、`words.tsv` と `words2.tsv` の clip 名から次のように `testdata/lat-short/` の名前へ移した（ja05 は ja30、ja06 は ja28、ja07 は ja29、ja08 は ja33、ja09 は ja27、ja10 は ja34、en01 は en02、en02 は en21、en03 は en22、en04 は en23、en05 は en24、en06 は en25、en07 は en26、en08 は en30、en09 は en31、en10 は en29。ja01 から ja04 は同じ名前）。
