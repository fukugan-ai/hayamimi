//! Which tier each language goes to, and what a missing tier gives way to.

use crate::route::tiers::TIER_FALLBACK;
use crate::route::{
    EnTier, PARA_LANGS, Policy, RZ_LANGS, RouteError, RouteNotice, SV_LANGS, Tier, V3_LANGS,
    fallback_chain, is_routable, requested_tier, resolve_en_tier,
};

use super::{SAMPLES, Script, router, single};

fn v2_opted_in() -> Policy {
    Policy {
        en_tier: EnTier::V2,
        ..Policy::single("en")
    }
}

#[test]
fn routing_sets_are_disjoint() {
    let sets: [&[&str]; 4] = [&RZ_LANGS, &PARA_LANGS, &SV_LANGS, &V3_LANGS];
    for (i, first) in sets.iter().enumerate() {
        for (j, second) in sets.iter().enumerate().skip(i + 1) {
            let shared = first.iter().find(|lang| second.contains(lang));
            assert!(
                shared.is_none(),
                "overlap between sets {i} and {j}: {shared:?}"
            );
        }
    }
}

#[test]
fn expected_language_homes() {
    assert!(RZ_LANGS.contains(&"ja"));
    // The Japanese model's English is unpunctuated capitals: English stays on v3.
    assert!(V3_LANGS.contains(&"en"));
    // The first identifier cannot say Cantonese, so SenseVoice arbitrates it.
    assert!(SV_LANGS.contains(&"yue"));
    assert!(PARA_LANGS.contains(&"zh"));
    assert!(
        ["ja", "zh", "ko", "yue", "en", "fr"]
            .into_iter()
            .all(is_routable)
    );
    assert!(!is_routable("xx"));
}

#[test]
fn resolve_en_tier_accepts_known_values() -> Result<(), RouteError> {
    assert_eq!(resolve_en_tier("v3")?, EnTier::V3);
    assert_eq!(resolve_en_tier("v2")?, EnTier::V2);
    Ok(())
}

#[test]
fn resolve_en_tier_rejects_unknown_value() {
    assert!(matches!(resolve_en_tier("v4"), Err(RouteError::Invalid(_))));
    assert!(matches!(resolve_en_tier(""), Err(RouteError::Invalid(_))));
}

#[test]
fn route_en_stays_on_v3_by_default() {
    assert_eq!(requested_tier("en", EnTier::V3), Tier::V3);
    assert_eq!(Policy::balanced().en_tier, EnTier::V3);
}

#[test]
fn route_en_switches_to_v2_when_opted_in() {
    assert_eq!(requested_tier("en", EnTier::V2), Tier::V2);
}

#[test]
fn route_other_v3_langs_unaffected_by_en_tier() {
    for lang in V3_LANGS.into_iter().filter(|lang| *lang != "en") {
        assert_eq!(requested_tier(lang, EnTier::V2), Tier::V3, "{lang}");
    }
}

#[test]
fn route_en_v2_missing_degrades_to_v3_not_rz() -> Result<(), RouteError> {
    let script = Script::present(&[Tier::Rz, Tier::Sv, Tier::V3, Tier::Omni]);
    let mut routed = router(script, None, v2_opted_in());
    let (_, used) = routed.decode_lang("en", &SAMPLES)?;
    assert_eq!(used, Tier::V3);
    assert_eq!(
        routed.take_notices(),
        [RouteNotice::Substituted {
            requested: Tier::V2,
            used: Tier::V3,
        }]
    );
    Ok(())
}

#[test]
fn route_en_v2_present_uses_v2() -> Result<(), RouteError> {
    let script = Script::present(&[Tier::Rz, Tier::V2, Tier::V3]);
    let mut routed = router(script, None, v2_opted_in());
    let (_, used) = routed.decode_lang("en", &SAMPLES)?;
    assert_eq!(used, Tier::V2);
    assert!(routed.take_notices().is_empty());
    Ok(())
}

#[test]
fn get_with_fallback_generic_chain_unchanged() -> Result<(), RouteError> {
    // A tier without an entry of its own keeps the chain that starts with Japanese.
    assert_eq!(
        fallback_chain(Tier::Pz),
        [Tier::Pz, Tier::Rz, Tier::Sv, Tier::V3, Tier::Omni]
    );
    let mut routed = single(Script::present(&[Tier::Rz]), "zh");
    let (_, used) = routed.decode_with_fallback(Tier::Pz, &SAMPLES)?;
    assert_eq!(used, Tier::Rz);
    Ok(())
}

#[test]
fn a_substitution_is_reported_once_per_requested_tier() -> Result<(), RouteError> {
    let mut routed = single(Script::present(&[Tier::Rz]), "zh");
    for _ in 0..3 {
        routed.decode_with_fallback(Tier::Pz, &SAMPLES)?;
    }
    assert_eq!(routed.take_notices().len(), 1);
    // A tier that is not installed is asked once; later segments go straight past it.
    assert_eq!(routed.decoder.times_asked(Tier::Pz), 1);
    Ok(())
}

#[test]
fn no_installed_tier_is_an_error_not_a_stop() {
    let mut routed = single(Script::present(&[]), "ja");
    assert!(matches!(
        routed.decode_with_fallback(Tier::Rz, &SAMPLES),
        Err(RouteError::NoModels)
    ));
}

#[test]
fn tier_fallback_table_only_names_known_tiers() {
    for (opt_in, defaults) in TIER_FALLBACK {
        assert!(Tier::ALL.contains(&opt_in));
        assert!(defaults.iter().all(|tier| Tier::ALL.contains(tier)));
    }
    let names: Vec<&str> = Tier::ALL.iter().map(|tier| tier.name()).collect();
    assert!(
        names
            .iter()
            .enumerate()
            .all(|(i, name)| !names[..i].contains(name))
    );
}
