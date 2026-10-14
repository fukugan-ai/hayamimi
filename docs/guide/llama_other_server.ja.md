# 2 本目の翻訳 server を起こす（ja と en の外の言語）

`hayamimi-relay live --translate zh` のように、ja と en の外の言語へ訳すには、TranslateGemma-4B-it を載せた 2 本目の llama-server が要ります。
1 本目（CAT-Translate-3.3b、port 8080）は ja と en の間だけを訳します。

この手順は WSL の user systemd で 2 本目を起こします。
Windows では、同じ model を `hayamimi llama start --model <file> --port 18081 --slots 1` で起こせます（[Windows の手順](llama_windows.ja.md)）。

## 起こす

前提は、llama.cpp の CUDA build と model の file が、unit の `ExecStart` に書いた path にあることです。
path が違うときは、unit の `-m` と `ExecStart` の先頭を直してから入れます。

```bash
mkdir -p ~/.config/systemd/user
install -m 0644 native/hayamimi-relay/units/hayamimi-llama-other.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user start hayamimi-llama-other.service
```

load が終わるまで数秒から 1 分ほどかかります（この機械の実測では、2 回目の確認で ok になりました）。
答えるようになったかを確かめます。

```bash
curl -s http://127.0.0.1:18081/health
```

出力は `{"status":"ok"}` です。
load の間は 503 が返ります。

## 使う

`live` と `eval` の `--llm-other` の既定は `http://127.0.0.1:18081` です。
そのまま打てます。

```bash
hayamimi-relay live --translate zh
```

port 8081 は、同じ機械の別の process が使っているので避けています。
別の port で起こすときは、unit の `--port` を直し、同じ address を `--llm-other http://127.0.0.1:<port>` で渡します。

## 立たないとき

2 本目が立っていなくても、中継は止まりません。
組の外の言語の行は `held`（訳の無い行）になり、log に `translate.request.failed`（翻訳器に届かなかった）が行ごとに出ます。
あわせて、最初の失敗で `translate.fallback.held` が 1 回だけ出て、`/events` にも `fallback`（`stage` が `translate`、`reason` が `unreachable`）が 1 件出ます。
届くようになると `translate.fallback.cleared` が 1 回出ます。
ja と en の訳は変わりません。

## 確かめる

2 本目を立てたまま、実 server に打つ test を走らせます。

```bash
cargo test --release -p hayamimi-relay --test chat_probe_gemma --test r8f_live -- --ignored
```

3 本の `chat_probe_gemma` は、中継の訳が `curl` で同じ body を打った訳と、40 文すべてで同じであることを確かめます。
`r8f_live` の 1 本は、ja の 1 文を `--translate zh` で通し、確定の訳が 1 件出ることを確かめます。

## 止める、戻す

```bash
systemctl --user stop hayamimi-llama-other.service
rm ~/.config/systemd/user/hayamimi-llama-other.service
systemctl --user daemon-reload
```

止めると GPU の memory が戻ります（この model は GPU の memory を 4629 MiB 使います。1000 MiB から 5629 MiB への実測です。1 本目と並べた値は RP-22 で測ります）。
