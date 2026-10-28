# BR-3 の IP-S21 を旗で外した値（速報に ITN と句読点を当てない run）

この file は、sherpa-onnx 2.0.1 と ONNX Runtime 1.31.1 の build（commit 7e2b82d。その後の rebase と 1 commit への畳み込みで hash が変わり、同じ変更は `feat/sherpa-2` の 1 本の commit に在る）で打った run の複写で、test の名前は `the_run_with_draft_marks_off_is_recorded`（step は `br03_ip_s21_off`）である。IP-S21 の記録 `br03_ip_s21_off.*`（sherpa-onnx 1.13.8）は書き直していない。この見出しの下の値は、run が書いたままで、手を入れていない。

設計 `docs/design/conversation_subtitles.ja.md` の 10.13.3（遅れの予算と測り方）と 10.13.7（受入 ID BR-3、T-BR-03-001）の線に対する値。合否は付けない。

## 段（BR-1。温まった標本は閉じる final を持つ発話の全部）

冷えた標本 2、温まった標本 76
d: n 76, p50 511 ms (線 300 ms) 線の外, p95 1061 ms (線 600 ms) 線の外
L_dec: n 76, p50 178 ms (線 270 ms) 線の内, p95 395 ms (線 480 ms) 線の内
L_tr: n 76, p50 90 ms (線 100 ms) 線の内, p95 311 ms (線 300 ms) 線の外
L_e: n 76, p50 234 ms, p95 405 ms
冷えた標本 repeat 1 utt 1 (ja): d 2021 ms, L_dec 180 ms, L_tr 307 ms
冷えた標本 repeat 1 utt 2 (en): d 407 ms, L_dec 354 ms, L_tr 1 ms

## 速報の訳 L_tr を言語で分けた値（BR-4。温まった標本）

L_tr ja: n 32, p50 200 ms, p95 337 ms
L_tr en: n 44, p50 2 ms, p95 258 ms

## 頁の段と頁までの遅れ（BR-2 の頁の時計と BR-1 を結んだ発話だけ）

結べた発話 78、結べなかった発話 0

L_pg: n 78, p50 1.000 ms (線 50 ms) 線の内, p95 1.900 ms (線 100 ms) 線の内
d: n 76, p50 511 ms (線 300 ms) 線の外, p95 1061 ms (線 600 ms) 線の外
L_dec: n 76, p50 178 ms (線 270 ms) 線の内, p95 395 ms (線 480 ms) 線の内
L_tr: n 76, p50 90 ms (線 100 ms) 線の内, p95 311 ms (線 300 ms) 線の外
L_page: n 76, p50 846 ms (線 750 ms) 線の外, p95 1359 ms (線 1300 ms) 線の外
L_settled_page: n 76, p50 1096 ms (線 1100 ms) 線の内, p95 1713 ms (線 1700 ms) 線の外
冷えた標本: n 2, `L_page` の最大 2512 ms (線 2500 ms) 線の外

## 頁の清書の欄と聞き取りの欄（BR-14、BR-15）

清書の行: 頁の行は 87 行で、清書の数 87 と同じ。届いた 90 件のうち、同じ id が重ねて届いた件は 3、うち script が流し直した件は 3
聞き取りの欄の確定の前: 欄の文が直前の速報と同じだった確定は 88 件、速報が無いか 150 ms に満たず比べなかった確定は 25 件
聞き取りの欄の確定の後: 速報の後の確定の次の確定で、間に速報が無く欄が待ちの文言だった確定は 4 件。最後の確定の後は、待ちの文言で、quiet の class を持つ
