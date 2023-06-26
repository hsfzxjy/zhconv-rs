//! This crate provides a ZhConverter that converts Chinese variants among each other. The
//! implementation is based on the [Aho-Corasick](https://docs.rs/aho-corasick/latest) automaton
//! with the leftmost-longest matching strategy and linear time complexity with respect to the
//! length of input text and conversion rules. It ships with a bunch of conversion tables,
//! extracted from [zhConversion.php](https://phabricator.wikimedia.org/source/mediawiki/browse/master/includes/languages/data/ZhConversion.php)
//! (maintained by MediaWiki and Chinese Wikipedia) and [OpenCC](https://github.com/BYVoid/OpenCC/tree/master/data/dictionary).
//!
//! While built-in rulesets work well for general case, the converter is never meant to be 100%
//! accurate, especially for professional text. In Chinese Wikipedia, it is pretty common for
//! editors to apply additional [CGroups](https://zh.wikipedia.org/wiki/Module:CGroup) and
//! [manual conversion rules](https://zh.wikipedia.org/wiki/Help:%E9%AB%98%E7%BA%A7%E5%AD%97%E8%AF%8D%E8%BD%AC%E6%8D%A2%E8%AF%AD%E6%B3%95)
//! on an article base. The converter optionally supports the conversion rule syntax used in
//! MediaWiki in the form `-{FOO BAR}-` and loading external rules defined line by line, which are
//! typically extracted and pre-processed from a [CGroup](https://zh.wikipedia.org/wiki/Category:%E5%85%AC%E5%85%B1%E8%BD%AC%E6%8D%A2%E7%BB%84%E6%A8%A1%E5%9D%97)
//! on a specific topic.
//! For simplicity, it is certainly also possible to add custom conversions by `(FROM, TO)` pairs.
//!
//! # Usage
//! This crate is [on crates.io](https://crates.io/crates/zhconv).
//! ```toml
//! [dependencies]
//! zhconv = "?"
//! ```
//!
//! # Example
//!
//! Basic conversion:
//! ```
//! use zhconv::{zhconv, Variant};
//! assert_eq!(zhconv("天干物燥 小心火烛", Variant::ZhHant), "天乾物燥 小心火燭");
//! assert_eq!(zhconv("鼠曲草", Variant::ZhHant), "鼠麴草");
//! assert_eq!(zhconv("阿拉伯联合酋长国", Variant::ZhHant), "阿拉伯聯合酋長國");
//! assert_eq!(zhconv("阿拉伯联合酋长国", Variant::ZhTW), "阿拉伯聯合大公國");
//! ```
//!
//! With MediaWiki conversion rules:
//! ```
//! use zhconv::{zhconv_mw, Variant};
//! assert_eq!(zhconv_mw("天-{干}-物燥 小心火烛", "zh-Hant".parse::<Variant>().unwrap()), "天干物燥 小心火燭");
//! assert_eq!(zhconv_mw("-{zh-tw:鼠麴草;zh-cn:香茅}-是菊科草本植物。", Variant::ZhCN), "香茅是菊科草本植物。");
//! assert_eq!(zhconv_mw("菊科草本植物包括-{zh-tw:鼠麴草;zh-cn:香茅;}-等。", Variant::ZhTW), "菊科草本植物包括鼠麴草等。");
//! assert_eq!(zhconv_mw("-{H|zh:馬;zh-cn:鹿;}-馬克思主義", Variant::ZhCN), "鹿克思主义"); // global rule
//! ```
//!
//! To load or add additional conversion rules such as CGroups or `(FROM, TO)` pairs,
//! see [`ZhConverterBuilder`].
//!

mod converter;
mod utils;

pub mod converters;
pub mod tables;

pub mod pagerules;
pub mod rule;
pub mod variant;

use self::utils::for_wasm;

for_wasm! {
    mod wasm;
}

pub use self::converter::{ZhConverter, ZhConverterBuilder};
pub use self::converters::get_builtin_converter;
use self::converters::*;
pub use self::tables::get_builtin_tables;
pub use self::variant::Variant;

/// Helper function for general conversion using built-in converters.
///
/// For fine-grained control and custom conversion rules, these is [`ZhConverter`].
#[inline(always)]
pub fn zhconv(text: &str, target: Variant) -> String {
    get_builtin_converter(target).convert(text)
}

/// Helper function for general conversion, activating inline conversion rules in MediaWiki syntax.
///
/// For general cases, [`zhconv`](#method.zhconv) should work well. Both of them share the same
/// built-in conversions tables.
///
/// # Note
/// Different from the implementation of MediaWiki, this crate use a automaton which makes it
/// infeasible to mutate global rules during converting. So the function always searches the text
/// for global rules such as `-{H|FOO BAR}-` in the first pass. If such rules exists, it build a
/// new converter from the scratch with built-in conversion tables, which **takes extra time**.
/// Otherwise, it just picks a built-in converter. Then it converts the text with the chosen
/// converter during when non-global rules are parsed and applied.
///
/// For fine-grained control and custom conversion rules, these is [`ZhConverter`].
pub fn zhconv_mw(text: &str, target: Variant) -> String {
    get_builtin_converter(target).convert_as_wikitext_extended(text)
}

/// Determine whether the given text looks like Simplified Chinese over Traditional Chinese.
///
/// Equivalent to `is_hans_probability(text) > 0.5`.
pub fn is_hans(text: &str) -> bool {
    is_hans_probability(text) > 0.5
}

/// Determine whether the given text looks like Simplified Chinese over Traditional Chinese.
///
/// The return value is a real number in the range `[0, 1]` (inclusive) that indicates
/// confidence level. A value close to 1 indicate high confidence. A value close to 0
/// indicates low confidence. `0.5` indicates undeterminable (half-half).
pub fn is_hans_probability(text: &str) -> f32 {
    let non_hant_score = ZH_TO_HANT_CONVERTER.count_matched(text) as f32;
    let non_hans_score = ZH_TO_HANS_CONVERTER.count_matched(text) as f32;
    // let mut ratio = if non_hans_score == 0 {
    //     f32::MAX
    // } else {
    //     non_hant_score as f32 / non_hans_score as f32
    // } - 1.0;
    // if ratio < 0.0 {
    //     ratio = -(1.0 / (ratio + 1.0) - 1.0);
    // }
    // 1f32 / (1f32 + E.powf(-ratio))
    non_hant_score / (non_hans_score + non_hant_score)
}

/// Determine the Chinese variant of the input text.
///
/// # Returns
/// Possible return values are only `ZhCN`, `ZhTW` and `ZhHK`.
pub fn infer_variant(text: &str) -> Variant {
    let non_cn_score = ZH_TO_CN_CONVERTER.count_matched(text);
    let non_tw_score = ZH_TO_TW_CONVERTER.count_matched(text);
    let non_hk_score = ZH_TO_HK_CONVERTER.count_matched(text);

    // authored by ChatGPT
    if non_cn_score <= non_tw_score && non_cn_score <= non_hk_score {
        Variant::ZhCN
    } else if non_tw_score <= non_cn_score && non_tw_score <= non_hk_score {
        Variant::ZhTW
    } else {
        Variant::ZhHK
    }
}

/// Determine the Chinese variant of the input text with confidence.
///
/// # Returns
/// A array of `(variant, confidence_level)`, where `confidence_level` is in the range `[0, 1]
/// (inclusive).
// /// Note that, unlike [`is_hans_confidence`](is_hans_confidence), a `confidence_level` greater
// /// than `0.5` might not imply high enough likelihood.
pub fn infer_variant_confidence(text: &str) -> [(Variant, f32); 5] {
    // let total = text.len() as f32;
    let non_cn_score = ZH_TO_CN_CONVERTER.count_matched(text) as f32;
    let non_tw_score = ZH_TO_TW_CONVERTER.count_matched(text) as f32;
    let non_hk_score = ZH_TO_HK_CONVERTER.count_matched(text) as f32;
    let non_hant_score = ZH_TO_HANT_CONVERTER.count_matched(text) as f32;
    let non_hans_score = ZH_TO_HANS_CONVERTER.count_matched(text) as f32;

    let total_score = non_cn_score + non_tw_score + non_hk_score - non_hant_score;
    // let region_total = non_cn_score + non_tw_score + non_hk_score - non_hant_score;
    // let script_total = non_hant_score + non_hans_score;
    let mut confidence_map = [
        (
            Variant::ZhCN,
            1f32 - non_cn_score.min(total_score) / total_score,
        ),
        (
            Variant::ZhTW,
            1f32 - non_tw_score.min(total_score) / total_score,
        ),
        (
            Variant::ZhHK,
            1f32 - non_hk_score.min(total_score) / total_score,
        ),
        (
            Variant::ZhHans,
            1f32 - non_hans_score.min(total_score) / total_score,
        ),
        (
            Variant::ZhHant,
            1f32 - non_hant_score.min(total_score) / total_score,
        ),
    ];
    // let mut confidence_map = [(Variant::ZhCN, 1f32 - non_cn_score / region_total),(Variant::ZhTW, 1f32 - non_tw_score / region_total),(Variant::ZhHK, 1f32 - non_hk_score / region_total),(Variant::ZhHans,1f32 - non_hans_score / script_total),(Variant::ZhHant, 1f32 - non_hant_score / script_total)];
    // let mut confidence_map = [(Variant::ZhCN, non_cn_score),(Variant::ZhTW, non_tw_score),(Variant::ZhHK, non_hk_score),(Variant::ZhHans,non_hans_score),(Variant::ZhHant, non_hant_score), (Variant::Zh, total)];

    // let mut confidence_map = [
    //     (Variant::ZhCN, 1f32 - non_cn_score / total),
    //     (Variant::ZhTW, 1f32 - non_tw_score / total),
    //     (Variant::ZhHK, 1f32 - non_hk_score / total),
    //     (Variant::ZhHans, 1f32 - non_hans_score / total),
    //     (Variant::ZhHant, 1f32 - non_hant_score / total),
    // ];
    confidence_map.sort_by(|a, b| b.1.total_cmp(&a.1));
    confidence_map
}
