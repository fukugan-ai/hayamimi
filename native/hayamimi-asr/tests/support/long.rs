//! The long material of `testdata/long_ja` and the way the tests that read it
//! count: three groups of three clips of `testdata/eval_real`, joined with
//! 0.2 s and with 0.5 s of silence between the clips.

use std::error::Error;
use std::path::{Path, PathBuf};

use hayamimi_asr::audio::read_wav;
use hayamimi_asr::{Recognizers, Tier};
use hayamimi_eval::cer::{Cer, cer, normalize_ja};

/// Threads of the recognizers the tests build.
pub const THREADS: i32 = 2;

/// A group of three clips, in the order they are joined.
pub struct Group {
    pub name: &'static str,
    pub clips: [&'static str; 3],
    pub refs: [&'static str; 3],
    /// Reference characters after normalisation.
    pub chars: usize,
    /// The sample count of the joined file for each gap of `GAPS`.
    pub lengths: [usize; 2],
}

pub const GROUPS: [Group; 3] = [
    Group {
        name: "L1",
        clips: ["ja_10", "ja_13", "ja_15"],
        refs: [
            "勝利はしたものの１０万人以上の戦死者を出していた。",
            "高校生２００人に聞きました。",
            "模擬裁判に参加した大学１年生の本音とは。",
        ],
        chars: 56,
        lengths: [323_218, 332_818],
    },
    Group {
        name: "L2",
        clips: ["ja_05", "ja_08", "ja_07"],
        refs: [
            "でも敦賀さん、９１歳力強い走りに変わっていきます。",
            "けさの東京、風はなさそうですけどね、体感はどうでしょうか、近藤さん。",
            "しっかり打ち切れるかです。",
        ],
        chars: 65,
        lengths: [321_116, 330_716],
    },
    Group {
        name: "L3",
        clips: ["ja_03", "ja_09", "ja_04"],
        refs: [
            "そこの上流から丸太を組んでいかだにしてどんどん流したんですこれを。",
            "そしてもう一つ大きな勢力があります。",
            "今、１００ｍのターンを迎える。",
        ],
        chars: 62,
        lengths: [329_519, 339_119],
    },
];

/// A silence between clips: its length in milliseconds (it names the file)
/// and in samples.
pub struct Gap {
    pub millis: u32,
    pub samples: usize,
}

pub const GAPS: [Gap; 2] = [
    Gap {
        millis: 200,
        samples: 3_200,
    },
    Gap {
        millis: 500,
        samples: 8_000,
    },
];

impl Group {
    /// The reference of the joined buffer: the three references, in order,
    /// with nothing between them.
    pub fn reference(&self) -> String {
        self.refs.concat()
    }

    /// The joined file of this group for `gap` under `testdata`.
    pub fn material(&self, testdata: &Path, gap: &Gap) -> PathBuf {
        testdata
            .join("long_ja")
            .join(format!("{}_g{}.wav", self.name, gap.millis))
    }

    /// What names the joined buffer of `gap` in a report.
    pub fn label(&self, gap: &Gap) -> String {
        format!("{} gap {} ms", self.name, gap.millis)
    }
}

/// An error measurement and the number of characters the decoded text holds.
#[derive(Clone, Copy, Default)]
pub struct Side {
    pub cer: Cer,
    pub chars: usize,
}

impl Side {
    pub fn plus(self, other: Side) -> Side {
        Side {
            cer: self.cer.plus(other.cer),
            chars: self.chars + other.chars,
        }
    }
}

/// The error of the whole-buffer decode of `samples` against `reference`.
pub fn measure(
    recognizers: &mut Recognizers,
    samples: &[f32],
    reference: &str,
) -> Result<Side, Box<dyn Error>> {
    let decoded = recognizers.decode_whole(Tier::Rz, samples)?;
    if decoded.tier != Tier::Rz {
        return Err(format!("the Japanese tier did not answer; {:?} did", decoded.tier).into());
    }
    Ok(Side {
        cer: cer(reference, &decoded.text),
        chars: normalize_ja(&decoded.text).len(),
    })
}

/// The samples of the clip `name` of `testdata/eval_real`.
pub fn eval_clip(testdata: &Path, name: &str) -> Result<Vec<f32>, Box<dyn Error>> {
    Ok(read_wav(
        &testdata.join("eval_real").join(format!("{name}.wav")),
    )?)
}

/// The clips decoded one by one, each against its own reference, summed.
pub fn measure_each(
    recognizers: &mut Recognizers,
    testdata: &Path,
    group: &Group,
) -> Result<Side, Box<dyn Error>> {
    let mut total = Side::default();
    for (name, reference) in group.clips.iter().zip(group.refs) {
        let samples = eval_clip(testdata, name)?;
        total = total.plus(measure(recognizers, &samples, reference)?);
    }
    Ok(total)
}
