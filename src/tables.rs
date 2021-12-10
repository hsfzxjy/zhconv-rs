use std::collections::HashMap;

use itertools;
use lazy_static::lazy_static;
use regex::Regex;

use crate::converter::ZhConverter;

/// Simplified Chinese to Traditional Chinese conversion table, including no region-specific phrases
pub const ZH_HANT_TABLE: (&'static str, &'static str) = (
    include_str!(concat!(env!("OUT_DIR"), "/zh2Hant.from.conv")),
    include_str!(concat!(env!("OUT_DIR"), "/zh2Hant.to.conv")),
);
/// Traditional Chinese to Simplified Chinese conversion table, including no region-specific phrases
pub const ZH_HANS_TABLE: (&'static str, &'static str) = (
    include_str!(concat!(env!("OUT_DIR"), "/zh2Hans.from.conv")),
    include_str!(concat!(env!("OUT_DIR"), "/zh2Hans.to.conv")),
);
/// Taiwan-specific phrases conversion table
pub const ZH_TW_TABLE: (&'static str, &'static str) = (
    include_str!(concat!(env!("OUT_DIR"), "/zh2TW.from.conv")),
    include_str!(concat!(env!("OUT_DIR"), "/zh2TW.to.conv")),
);
/// Hong Kong-specific phrases conversion table
pub const ZH_HK_TABLE: (&'static str, &'static str) = (
    include_str!(concat!(env!("OUT_DIR"), "/zh2HK.from.conv")),
    include_str!(concat!(env!("OUT_DIR"), "/zh2HK.to.conv")),
);
/// Macau-specific phrases conversion table
pub const ZH_MO_TABLE: (&'static str, &'static str) = ZH_HK_TABLE;
/// Mainland China-specific phrases conversion table
pub const ZH_CN_TABLE: (&'static str, &'static str) = (
    include_str!(concat!(env!("OUT_DIR"), "/zh2CN.from.conv")),
    include_str!(concat!(env!("OUT_DIR"), "/zh2CN.to.conv")),
);
/// Mainland Singapore-specific phrases conversion table
pub const ZH_SG_TABLE: (&'static str, &'static str) = ZH_CN_TABLE;
/// Mainland Singapore-specific phrases conversion table
pub const ZH_MY_TABLE: (&'static str, &'static str) = ZH_SG_TABLE;

// Ref: https://github.com/wikimedia/mediawiki/blob/6eda8891a0595e72e350998b6bada19d102a42d9/includes/language/converters/ZhConverter.php#L157
lazy_static! {
    /// For `ZH_TO_TW_CONVERTER`, merged from `ZH_HANT_TABLE` and `ZH_TW_TABLE`
    pub static ref ZH_HANT_TW_TABLE: (&'static str, &'static str) =
        merge_tables_leaked(ZH_TW_TABLE, ZH_HANT_TABLE);
    /// For `ZH_TO_HK_CONVERTER`, merged from `ZH_HANT_TABLE` and `ZH_HK_TABLE`
    pub static ref ZH_HANT_HK_TABLE: (&'static str, &'static str) =
        merge_tables_leaked(ZH_HK_TABLE, ZH_HANT_TABLE);
    /// For `ZH_TO_MO_CONVERTER`, merged from `ZH_HANT_TABLE` and `ZH_MO_TABLE`
    pub static ref ZH_HANT_MO_TABLE: (&'static str, &'static str) =
        merge_tables_leaked(ZH_MO_TABLE, ZH_HANT_TABLE);
    /// For `ZH_TO_CN_CONVERTER`, merged from `ZH_HANS_TABLE` and `ZH_CN_TABLE`
    pub static ref ZH_HANS_CN_TABLE: (&'static str, &'static str) =
        merge_tables_leaked(ZH_CN_TABLE, ZH_HANS_TABLE);
    /// For `ZH_TO_CN_CONVERTER`, merged from `ZH_HANS_TABLE` and `ZH_SG_TABLE`
    pub static ref ZH_HANS_SG_TABLE: (&'static str, &'static str) =
        merge_tables_leaked(ZH_SG_TABLE, ZH_HANS_TABLE);
    /// For `ZH_TO_CN_CONVERTER`, merged from `ZH_HANS_TABLE` and `ZH_MY_TABLE`
    pub static ref ZH_HANS_MY_TABLE: (&'static str, &'static str) =
        merge_tables_leaked(ZH_MY_TABLE, ZH_HANS_TABLE);
}

// TODO: How to make these lazy consts more idiomatic?

fn merge_tables_leaked(conv1: (&str, &str), conv2: (&str, &str)) -> (&'static str, &'static str) {
    let (froms, tos) = merge_tables(conv1, conv2);
    (
        Box::leak(froms.into_boxed_str()),
        Box::leak(tos.into_boxed_str()),
    )
}

/// Merge two conversion table
pub fn merge_tables(conv1: (&str, &str), conv2: (&str, &str)) -> (String, String) {
    let mut froms = String::with_capacity(conv1.0.len() + conv2.0.len());
    let mut tos = String::with_capacity(conv1.1.len() + conv2.1.len());
    let mut it = itertools::Itertools::merge_by(
        itertools::zip(conv1.0.trim().split("|"), conv1.1.trim().split("|")),
        itertools::zip(conv2.0.trim().split("|"), conv2.1.trim().split("|")),
        |pair1, pair2| pair1.0.len() >= pair2.0.len(),
    )
    .peekable();
    while let Some((from, to)) = it.next() {
        froms.push_str(from);
        tos.push_str(to);
        if it.peek().is_some() {
            froms.push_str("|");
            tos.push_str("|");
        }
    }
    return (froms, tos);
}

// pub const ZH_HANT_TO: &str = include_str!(concat!(env!("OUT_DIR"), "/zh2Hant.to.conv"));

// pub const ZH_HANS_FROM: &str = include_str!(concat!(env!("OUT_DIR"), "/zh2Hans.from.conv"));
// pub const ZH_HANS_TO: &str = include_str!(concat!(env!("OUT_DIR"), "/zh2Hans.to.conv"));

// pub const ZH_TW_FROM: &str = include_str!(concat!(env!("OUT_DIR"), "/zh2TW.from.conv"));
// pub const ZH_TW_TO: &str = include_str!(concat!(env!("OUT_DIR"), "/zh2TW.to.conv"));

// pub const ZH_HK_FROM: &str = include_str!(concat!(env!("OUT_DIR"), "/zh2HK.from.conv"));
// pub const ZH_HK_TO: &str = include_str!(concat!(env!("OUT_DIR"), "/zh2HK.to.conv"));

// pub const ZH_CN_FROM: &str = include_str!(concat!(env!("OUT_DIR"), "/zh2CN.from.conv"));
// pub const ZH_CN_TO: &str = include_str!(concat!(env!("OUT_DIR"), "/zh2CN.to.conv"));

/// Build a `ZhConverter` from a conversion table
pub fn build_converter((froms, tos): (&str, &str)) -> ZhConverter {
    // dbg!(froms, tos);
    let p = Regex::new(froms).unwrap();
    let m: HashMap<String, String> = itertools::zip(froms.trim().split("|"), tos.trim().split("|"))
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect();
    // dbg!(&p,&m);
    ZhConverter::new(p, m)
}

// https://github.com/wikimedia/mediawiki/blob/6eda8891a0595e72e350998b6bada19d102a42d9/includes/language/converters/ZhConverter.php#L144