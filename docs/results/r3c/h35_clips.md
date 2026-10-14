# H35 の仕分け：zh と yue が Python より劣る差

RP-1 で R3b の Rust の zh は 0.0699、yue は 0.0682 で、Python の 0.066 と 0.061 を超えていた。
この文書は、clip ごとに Rust と Python の認識文を並べて差の原因を仕分けた結果である。

## 結論

差は Rust の code（SenseVoice と Paraformer の設定、ITN、t2s、採点）には無い。
Python と同じ native library（sherpa-onnx 1.13.6 に同梱の ONNX Runtime 1.27.1、Windows）を Rust の同じ code に読ませると、全 66 clip（ja 15、en 15、zh 12、ko 12、yue 12）の誤り率が Python の `scorecard_run1.stdout.log` と小数 3 桁ですべて一致する。
差を作っているのは、認識器を動かす native library の build（ONNX Runtime の版と OS）で、int8 の model が境界上の 1 文字を別の字にする。
ITN の差、t2s の差、Rust の句読点の差は、この評価組では 0 件である。

yue だけは別の理由で、Python と同じ library を使っても 0.061454 になり、`0.061` を超える。
Python の表は小数 3 桁に丸めた値で、Python 自身の丸める前の値が 0.061454 だからである。
丸めた値を閾値にする限り、Python と同じ性能の engine は yue の RP-1 を通らない。
この点の直し方は設計の判断で、この slice では決めない（末尾の「設計の判断が要る点」）。

## 環境ごとの RP-1 の値

誤り率は clip の長さで重みを付けた平均（百万分率）。
認識の code（`hayamimi-asr`）はどの環境でも同じである。

| 言語 | Python の表 | A. WSL、自前 build（ORT 1.28.2） | B. Windows、自前 build（ORT 1.31.0） | C. Windows、Python と同じ library（ORT 1.27.1） |
|---|---|---|---|---|
| ja | 38000 | 37512 | 37512 | 37512 |
| en | 23000 | 22606 | 22606 | 22606 |
| zh | 66000 | 69933（超過） | 65609 | 65609 |
| ko | 81000 | 79278 | 86939（超過） | 80572 |
| yue | 61000 | 68161（超過） | 61454（超過） | 61454（超過） |

A は R3b の run（`wsl_self_built.json`）を同じ commit で打ち直した値で、R3b の記録と一致する。
B と C は、この slice で `hayamimi-eval scorecard` が Windows で動くようにしてから測った値である（`win_self_built.json`、`win_python_libs.json`）。
ja と en は 3 環境とも同じ値で、Python 以下である。

ORT の版は `hayamimi_gpu::runtime::onnxruntime_version` で読んだ値（B と C）と、sherpa-onnx 1.13.8 の CHANGELOG の記述（A）である。
A の実行時の版は測っていない。

## 差の件数

対象は WSL の run（A）と、Python と同じ library の run（C）で文が違う clip である。
全 66 clip（ja 15、en 15、zh 12、ko 12、yue 12）のうち、文が違うのは 6 clip である。

| 仕分け | 件数 | clip |
|---|---|---|
| ITN の差 | 0 | |
| Rust の句読点の差 | 0 | |
| t2s の差（H24） | 0 | |
| 認識器の出力の 1 文字の差（native library の数値差） | 5 | zh_09、yue_04、ko_01、ko_03、ko_12 |
| 認識器が自分で付けた読点の有無（誤り率に効かない） | 1 | yue_12 |

ja と en の 30 clip は 2 環境で文が同じである。

H24 の t2s は、文字列の差を直接は見ていない。
Python の OpenCC は動かしていないので、Rust の `ferrous-opencc` と Python の `opencc` の t2s を同じ入力に通した差は NOT VERIFIED である。
効果としては、C の yue 12 本の誤り率が Python の 12 本と一致するので、この組の参照文と認識文では t2s の差が誤り率に出ていない。

## clip ごとの文

「Python 相当の文」は、Python と同じ native library で Rust が出した文で、誤り率が Python の log と一致する。
Python 自身の文は log に残っていない。
zh_09 だけは、Python の評価記録 `docs/eval/eval_real_zhko.md` の Paraformer の文が「然后拉卡辛格带头唱了半赞歌」で、C の文と一致する。

括弧の数は Rust の誤り率。
Python の列は `scorecard_run1.stdout.log` の clip ごとの誤り率で、Rust の誤り率と同じなら「一致」。

### zh（Paraformer、tier pz）

| clip | Rust（WSL）の文 | Python 相当の文 | Python の誤り率 | 仕分け |
|---|---|---|---|---|
| zh_01 | 因为远离大陆哺乳动物无法长途发射而来使得巨龟成为克隆群岛主主要的食草动物（0.114） | 同じ | 0.114 | 一致 |
| zh_02 | 亚马逊河也是地球上最宽的河流有些河道的宽度可达六英里（0.077） | 同じ | 0.077 | 一致 |
| zh_03 | 他称他制作了一个 w i i 门铃（0.143） | 同じ | 0.143 | 一致 |
| zh_04 | 新国王时期的古埃及人惊叹于其前辈的已有1000多年的历史的纪念碑（0.241） | 同じ | 0.241 | 一致 |
| zh_05 | 两小时内政府大楼附近又发生了三起炸弹爆炸事件（0） | 同じ | 0 | 一致 |
| zh_06 | 一般来说卫星电话不能取代移动电话因为只有在卫星信号畅通的室外才能进行通话（0） | 同じ | 0 | 一致 |
| zh_07 | 西班牙人开始了长达三个世纪的殖民时期（0） | 同じ | 0 | 一致 |
| zh_08 | 内陆水稻可以作为假期游玩的一个不错的主题（0.050） | 同じ | 0.050 | 一致 |
| zh_09 | 然后拉卡辛格带头唱了半唱歌（0.214） | 然后拉卡辛格带头唱了半赞歌（0.143） | 0.143 | 認識器の 1 文字の差（赞と唱）。Rust の側の誤りではない |
| zh_10 | 西班牙人开始了长达三个世纪的殖民时期（0） | 同じ | 0 | 一致 |
| zh_11 | 绘图分析的结果将发布在公网上（0） | 同じ | 0 | 一致 |
| zh_12 | 有些音乐节会为带小孩子的家庭设立特别的露营区（0.048） | 同じ | 0.048 | 一致 |

zh_04 の「1000」は ITN が「一千」を数字にした結果で、参照文は「一千」なので誤りに数える。
Python の ITN も同じ規則で同じ誤りを出している（Python の誤り率 0.241）。
これは Python の意図どおりの動作で、直す対象ではない。

### yue（SenseVoice、tier sv）

| clip | Rust（WSL）の文 | Python 相当の文 | Python の誤り率 | 仕分け |
|---|---|---|---|---|
| yue_01 | 如果你在冬季造访北极或南极地区你会经历永夜亦即太阳不会升到地平线以上。（0） | 同じ | 0 | 一致 |
| yue_02 | 这些理论认为人们有一些需求或渴望在长大成人的过程中内化了。（0） | 同じ | 0 | 一致 |
| yue_03 | 咖喱依据液体含量多寡而有肝或湿之分。（0.118） | 同じ | 0.118 | 一致 |
| yue_04 | 然后由拉卡新领盒大唱败赞歌。（0.308） | 然后由拉卡新领侠大唱拜赞歌。（0.231） | 0.231 | 認識器の 2 文字の差（盒と侠、败と拜）。Rust の側の誤りではない |
| yue_05 | 值得在这座迷人村庄漫步半小时。（0） | 同じ | 0 | 一致 |
| yue_06 | 他们仍在试着判断撞击的规模以及对地球的影响。（0.048） | 同じ | 0.048 | 一致 |
| yue_07 | 一旦你脱离了洋楼游回岸并不比平时困难。（0.111） | 同じ | 0.111 | 一致 |
| yue_08 | 对我来说这似乎不合理这肯定不公平。（0） | 同じ | 0 | 一致 |
| yue_09 | 其无所不在的能力影响所有人上至国王下至平民。（0） | 同じ | 0 | 一致 |
| yue_10 | 他也没有权力撤销州与州之间的税法和关税。（0.053） | 同じ | 0.053 | 一致 |
| yue_11 | 针对有幼童的家庭有些节庆会场入会场会安排专属的露营区域。（0.125） | 同じ | 0.125 | 一致 |
| yue_12 | 阿妈逊河也是地球上最宽的河流有些河段能达到六英里宽。（0.080） | 阿妈逊河也是地球上最宽的河流，有些河段能达到六英里宽。（0.080） | 0.080 | 認識器が付けた読点の有無。誤り率は同じ |

### ko（SenseVoice、tier sv）で文が違う clip

ko は H35 の対象ではないが、同じ原因を確かめるために文の違う 3 clip を並べる。
ko の平均は A で 0.0793、C で 0.0806 で、どちらも Python の 0.081 以下である。

| clip | Rust（WSL）の文 | Python 相当の文 | Python の誤り率 | 仕分け |
|---|---|---|---|---|
| ko_01 | 염소사육은 대략 1만 년 전에 이란의 자그로스 산맥에서 시작한 것으로 보입니다.（0.030） | 염소사육은 대략  일만 년 전에 이란의 자그로스 산맥에서 시작한 것으로 보입니다.（0） | 0 | 認識器の出力の差（数字の書き方） |
| ko_03 | 교전이 발한 직후 영국은 독일에 대한 해상봉세를 시작한다.（0.080） | 교전이 발한 직후 영국은 독일에 대한 해상공세를 시작한다.（0.120） | 0.120 | 認識器の 1 文字の差。この clip は Rust が良い |
| ko_12 | 19백40년8월 15일 연합군은 프랑스 남부를 침략했고 이 침략은 드래곤 작전이라 불렸다.（0.053） | 19백40년8월 15일 연합권은 프랑스 남부를 침략했고 이 침략은 드래곤 작전이라 불렸다.（0.079） | 0.079 | 認識器の 1 文字の差。この clip は Rust が良い |

差の向きは clip によって逆である。
どちらの向きに偏った差でもなく、境界上の字が library の数値の違いで入れ替わる形である。

## 原因を native library だと言える根拠

1. A（WSL、ORT 1.28.2）と C（Windows、ORT 1.27.1）は、同じ commit の Rust が同じ設定（SenseVoice は言語 auto と ITN あり、Paraformer は貪欲探索、スレッド 6）で動いた結果で、違うのは library だけである。
2. C の 66 clip の誤り率は、Python の log と 66 本すべて一致する。A は 5 本で違う。
3. 設定の側を動かしても A は Python の値にならない。SenseVoice の言語指定（auto、空、clip の言語、zh、en、ja、ko、yue）と ITN（あり、なし）の 16 通りを A で打ち直したところ、Python の ko と yue の 24 本の誤り率に全部は一致する組が無く、最も近い組でも 21 本だった（既定の auto と ITN ありは 20 本）。
4. 同じ Rust を B（ORT 1.31.0）で動かすと、zh と yue は C と同じ値になり、ko は ko_03 と ko_05 が悪くなって 0.0869 になる。library を替えるたびに、境界上の別の clip が入れ替わる。
5. スレッド数は結果を変えない。zh_09 は 1、2、4、6 スレッドのどれでも WSL では「半唱歌」だった。

## 比べ方と測る build

1. **丸め**：Python の表は小数 3 桁で、Python 自身の yue は 0.061454 である。
   `check_scorecard` は Rust の平均を小数 3 桁へ四捨五入してから比べる（`scorecard.rs` の `thousandths`）。R11 の入口の ko は丸めず 86939 と整数で比べる(10 の H35)。
   0.061454 は 0.061 になって通り、0.0615 は 0.062 で落ちる。閾値は動かしていない。
2. **測る build**：RP-1 の合否は、配る形の build（自前の sherpa-onnx と ORT 1.31.0、Windows）で決める。
   版を Python に合わせて古くする手は、依存を最新に保つ規則に反するので採らない。
3. **LID の数**：`hayamimi-eval scorecard` は言語を clip の正解で決める（`lid: oracle`）ので、RP-1 の LID の列は空のままで、T-RP-1-012 は言語判定を繋ぐ slice が済むまで通らない。

## 配る形の build の RP-1

`win_self_built.json` は、merge 後の code で打ち直した値である。

| 言語 | Rust（百万分率） | 丸め後 | Python の表 | 判定 |
|---|---|---|---|---|
| ja | 37512 | 38000 | 38000 | 通る |
| en | 22606 | 23000 | 23000 | 通る |
| zh | 65609 | 66000 | 66000 | 通る |
| ko | 86939 | 87000 | 81000 | 超える（H35） |
| yue | 61454 | 61000 | 61000 | 通る |

## 振り分けを通した run

`win_routed.json` は、配る形の build（Windows、自前の sherpa-onnx と ORT 1.31.0）で、`hayamimi-asr` の振り分けに clip の言語判定をさせた run である（`lid: detected`、clip の正解の言語は渡していない）。
build は、この branch に R3b の 8fe1f19（scorecard を振り分けに繋ぐ変更）を足した検証用の branch で行った。

| 言語 | 言語判定の正解 | 誤り率（百万分率、振り分けを通した） | 正解の言語を渡した run | 丸め後 | Python の表 | 判定 |
|---|---|---|---|---|---|---|
| ja | 15/15 | 37512 | 37512 | 38000 | 38000 | 通る |
| en | 15/15 | 22606 | 22606 | 23000 | 23000 | 通る |
| zh | 12/12 | 65609 | 65609 | 66000 | 66000 | 通る |
| ko | 12/12 | 86939 | 86939 | 87000 | 81000 | 超える |
| yue | 12/12 | 61454 | 61454 | 61000 | 61000 | 通る |

66 clip すべてで言語判定が当たった。
言語判定の外れた clip が無いので、言語判定の外れの分と、正しい言語で読んだ clip の分の差は 0 である。
zh と yue は Python を超えない。ja_02 は ja と判定され、誤り率は 0 である（session の最初の clip は ja_01 で、1 set に session は 1 回）。
R3b の申告にあった zh 0.0699 と yue 0.0682 は、この build では出ない。

## ko の fp32

SenseVoice を CPU の fp32 で読ませた。
sherpa-onnx の `asr-models` にある `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17` の `model.onnx` を、int8 の dir と同じ名前の dir に置いた別の models 根を渡し、code と build は配る形のままにした（`win_sv_fp32.json`）。

| 言語 | 誤り率 int8（配る形） | 誤り率 fp32 | RTF int8 から fp32 |
|---|---|---|---|
| ko | 86939 | 93700 | 68450 から 135249 |
| yue | 61454 | 91235 | 40902 から 59622 |

ko は fp32 でも 0.081 に届かず、0.0937 と悪くなる。
時間は RTF で約 2 倍になる。fp32 に替える条件（ko が 0.081 以下で、`L` の延びが 1% 以内）はどちらも満たさない。
ko の 12 clip のうち、int8 と fp32 で誤り率が違うのは 6 clip である。
ko_01（0 から 0.030）、ko_03（0.160 から 0.120）、ko_05（0.192 から 0.154）、ko_08（0 から 0.125）、ko_11（0.111 から 0.083）、ko_12（0.079 から 0.053）で、fp32 は良くなる clip もあるが、別の clip の字を外す。
配る形（B）と Python と同じ library（C）で誤り率が違う ko は、ko_03 と ko_05 の 2 clip である。

## ja_02 の SenseVoice（H40）

`Recognizer::open_tier(Tier::Sv)` と `decode_tagged` を呼ぶだけの probe で、ja_02 を読ませた。

| build | ORT | SenseVoice の答え |
|---|---|---|
| 配る形（B） | 1.31.0 | zh（追卡飞卡卡。） |
| Python と同じ library（C） | 1.27.1 | zh（飞卡皮卡追卡。） |

ja_02 は、Python の wheel の ORT でも zh と答える。ORT の build 差（H35）から出た件ではない。
入力は 16 kHz の wav で、Rust も Python も resample せずに渡す。
Python の `eval_engine.py` の clip の順は ja_01、ja_02 の順で、最初の ja_01 で SenseVoice の probe が走り、session の言語は ja になる。
ja_02 では whisper-tiny の答え（ja）が session の言語と同じなので、`asr_engine.py` の `transcribe` は SenseVoice を呼ばない。
Rust 側で ja_02 が zh になるのは、ja_02 を session の最初の clip として SenseVoice に通した場合である。この読みは ASSUMPTION で、言語判定を繋いだ run（R3b、H37）を clip の順どおりに打って確かめる。

## 打った command

WSL（A）。

```
cd <repo>
<worktree>/native/target/release/hayamimi-eval scorecard --engine hayamimi \
  --sets testdata/eval_real,testdata/eval_real_zhko,testdata/eval_real_yue \
  --out docs/results/r3c/wsl_self_built.json
```

Windows（B と C）。
`SHERPA_ONNX_LIB_DIR` に、B は自前 build の `native\target\sherpa-gpu\windows\install\lib`、C は `.venv\Lib\site-packages\sherpa_onnx\lib` の `sherpa-onnx-c-api.dll` と `.lib`、`onnxruntime.dll` に、自前 build の `onnxruntime.lib` を足した directory を渡して build し、実行 file の隣に同じ DLL を置く。

```
cargo build --release --target x86_64-pc-windows-msvc -p hayamimi-eval
hayamimi-eval.exe scorecard --engine hayamimi \
  --sets testdata/eval_real,testdata/eval_real_zhko,testdata/eval_real_yue \
  --out docs/results/r3c/win_self_built.json
```

C の library は Python の同梱物の読み取りだけで、Python の file には何も書いていない。

振り分けを通した run は、R3b の 8fe1f19 を足した検証用の branch で、B と同じ library と同じ引数のまま `--out docs/results/r3c/win_routed.json` に変えて打った。
