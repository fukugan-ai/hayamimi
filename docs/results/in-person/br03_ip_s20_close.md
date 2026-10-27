# BR-3 の IP-S20 を旗で外した値（確定文の言語を閉じる時に決める）

設計 `docs/design/conversation_subtitles.ja.md` の 10.13.3（遅れの予算と測り方）と 10.13.7（受入 ID BR-3、T-BR-03-001）の線に対する値。合否は付けない。

## 段（BR-1。温まった標本は閉じる final を持つ発話の全部）

冷えた標本 2、温まった標本 76
d: n 76, p50 507 ms (線 300 ms) 線の外, p95 1062 ms (線 600 ms) 線の外
L_dec: n 76, p50 206 ms (線 270 ms) 線の内, p95 484 ms (線 480 ms) 線の外
L_tr: n 75, p50 50 ms (線 100 ms) 線の内, p95 325 ms (線 300 ms) 線の外
L_e: n 75, p50 235 ms, p95 403 ms
冷えた標本 repeat 1 utt 1 (ja): d 2030 ms, L_dec 214 ms, L_tr 239 ms
冷えた標本 repeat 1 utt 2 (en): d 419 ms, L_dec 456 ms, L_tr 2 ms

## 速報の訳 L_tr を言語で分けた値（BR-4。温まった標本）

L_tr ja: n 31, p50 178 ms, p95 352 ms
L_tr en: n 44, p50 4 ms, p95 255 ms

## 頁の段と頁までの遅れ（BR-2 の頁の時計と BR-1 を結んだ発話だけ）

結べた発話 77、結べなかった発話 1

L_pg: n 77, p50 1.100 ms (線 50 ms) 線の内, p95 1.900 ms (線 100 ms) 線の内
d: n 75, p50 507 ms (線 300 ms) 線の外, p95 1062 ms (線 600 ms) 線の外
L_dec: n 75, p50 206 ms (線 270 ms) 線の内, p95 484 ms (線 480 ms) 線の外
L_tr: n 75, p50 50 ms (線 100 ms) 線の内, p95 325 ms (線 300 ms) 線の外
L_page: n 75, p50 908 ms (線 750 ms) 線の外, p95 1384 ms (線 1300 ms) 線の外
L_settled_page: n 75, p50 1146 ms (線 1100 ms) 線の外, p95 1701 ms (線 1700 ms) 線の外
冷えた標本: n 2, `L_page` の最大 2486 ms (線 2500 ms) 線の内
結べなかった: repeat 1 utt 13: file-17: the page recorded no translation
