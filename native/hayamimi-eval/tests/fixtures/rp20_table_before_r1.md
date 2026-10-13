### 3.5 段ごとの置き場

既定は GPU である。
この表は R1 の前の写しで、「いまの置き場」の列に `測る` を含む行が 8 件ある。

| 段 | いまの置き場 | 根拠、または R1 で測る物 |
|---|---|---|
| 音声の取り込み（WASAPI のマイク、loopback） | CPU | 道具が無い。WASAPI の capture は host の buffer に sample を渡す（S7）。R1 では取り込みから認識器へ渡すまでの時間を測る |
| int16 → float32、16 kHz への resample、mix | 測る | sherpa-onnx の C の API が host の `const float *` で音声を受ける（S11）。R1 で CPU の時間と、GPU の時間と往復の転送の和を比べる |
| 音声の ring buffer と preroll（1.0 秒） | CPU | 同上の理由で、認識器へは host の配列で渡す（S11） |
| VAD（Silero、sherpa-onnx の `provider`） | 測る | Silero の README は CPU の 1 thread で 30 ms の塊が 1 ms 未満と書く（S8） |
| 言語判定（whisper-tiny） | GPU | R1 で GPU と CPU を測り、GPU が遅ければ表を直す |
| 認識の encoder（全 model） | GPU | 同上 |
| fbank と探索（sherpa-onnx の中） | sherpa-onnx の実装のまま | R1 で、1 発話の認識の時間を「fbank」「encoder」「探索」に分けて測る |
| 冒頭欠落の疑い判定と分割再試行 | GPU（解き直し）。判定は測る | 判定は文字数と秒数の比較 1 回。R1 で 1 行の時間を記録する |
| ITN（漢数字 → 算用数字） | 測る | 文字列の正規表現の処理。R1 で 1 行の時間を記録する |
| 句読点（BERT、`ort` の CUDA） | GPU | 入力の文字列 → token id の変換は表を引く 1 回。R1 で GPU と CPU を測る |
| 句読点の後処理と疑問符の規則、置換辞書 | 測る | 文字列の規則。ITN と同じ扱い |
| 話者の埋め込み（CAM++、sherpa-onnx の `provider`） | GPU | R1 で測る |
| 話者の速報の割り当て（cosine と重心の更新） | 測る | 重心は数個から数十個。R1 で 1 発話の時間を記録する |
| 清書の再分離（pyannote、sherpa-onnx の `provider`） | GPU | R1 で測る |
| 清書の clustering（FastClustering）と全体の対応づけ | 測る | 1 群あたり埋め込みは数個から数十個 |
| 訳 T と E（llama-server、CUDA） | GPU | R1 で T の 1 回の時間を GPU と CPU で測る |
| 認識の文字列 → 訳の入力 | CPU（文字列） | 認識の token と言語モデルの token は語彙が違い、文字列に戻して切り直すしかない（S14） |
| 照合 V と単位の換算 | 測る | 規則と表の文字列処理。R1 でも同じ 40 行の時間を記録する |
| 字幕の配信（SSE）と描画 | CPU（server）、描画は WebView2 | 文字列の配信で、GPU の道具は無い |

### 3.6 次の節
