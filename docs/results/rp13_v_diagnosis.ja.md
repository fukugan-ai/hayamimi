# RP-13 の訳と V の止めすぎの診断

V を掛けなければ、4 言語とも訳は RP-13 の記録を超える。記録を割るのは、V が正しい訳まで止めて、その行が空の hyp になるからである。

条件は次のとおり。
文は `testdata/baseline/pairs/` の FLEURS 50 組（`fleurs-pairs --n 50 --seed 0`）。
訳は ja から各言語へ、T だけ（E も V も掛けない）。en は CAT-Translate-3.3b、zh、ko、es は TranslateGemma-4B-it Q8_0（ja→zh は ja→ko、ja→es と同じ形）。
V は `hayamimi-relay` の組み込みの表（native `Verifier::verify_into`）。
測った日は 2026-10-14。coder 席の使い捨ての crate で測り、repo には入れていない。

## V を掛けない全行の chrF

`hayamimi-eval chrf --pairs testdata/baseline/pairs --hyps <T だけの hyp>` の出力。

| 言語 | 記録 | V なし | V あり、止めた行は空（`rp13_translation.md`） |
|---|---|---|---|
| en | 47.57 | 54.29 | 44.56 |
| es | 42.09 | 45.79 | 35.87 |
| ko | 25.20 | 27.76 | 22.78 |
| zh | 20.19 | 28.78 | 16.52 |

「V あり」の en は T の後に E を掛けた訳で、「V なし」の en は T の下書きだけである。
2 つの列は同じ訳ではないので、差のすべてが V の効果ではない。

## 止めた行の理由

どの行も 50 行中の数。数は、V を掛けた各行の判定を理由ごとに数えた。

| 言語 | 訳（T だけ）の止めた行 | 参照訳の止めた行 |
|---|---|---|
| en | 15（uncertainty 6、number 4、negation 2、form 2、proper_noun 1） | 14（number 6、negation 4、uncertainty 3、proper_noun 1） |
| zh | 18（negation 7、number 6、uncertainty 4、proper_noun 1） | 20（number 8、negation 7、uncertainty 4、proper_noun 1） |
| ko | 13（negation 8、uncertainty 2、number 2、form 1） | 14（negation 6、uncertainty 4、number 2、form 1、proper_noun 1） |
| es | 15（number 10、uncertainty 3、negation 1、form 1） | 22（number 15、uncertainty 3、negation 2、proper_noun 2） |

参照訳は人手の正しい訳である。参照訳が 14 から 22 行止まるので、V は正しい訳を 28% から 44% 止めている。
会話字幕の CS-24 は、この数を 0 にする条件である。

## 表では直せない 2 つの型

評議へ戻す型の数。行は pairs の行番号。

| 型 | 参照訳 | 訳（T だけ） |
|---|---|---|
| 含意の否定「もってのほか」（原文に否定の標識が無く、訳が `Do not` や「不要」のように否定語で書く） | 3 行（en、zh、es の 18 行目。ko は止まらない） | 1 行（zh の 18 行目） |
| 含意の否定 `failed to`（原文は `上がれませんでした` で否定の標識があり、訳の `failed to` は否定語の表に無い） | 1 行（en の 48 行目） | 0 行 |
| 伝聞の「そうです」（原文の `そうです` を推量の標識に数え、訳に推量の語が出ない） | 3 行（en、zh、es の 50 行目。ko は止まらない） | 3 行（en、zh、es の 50 行目） |

3 つの型は、原文側か訳の側の一方にしか標識が出ないので、標識の表を足しても直らない。
表で足せる範囲は別の作業で調べる。この文書は型ごとの数だけを残す。
