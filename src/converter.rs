use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::iter::IntoIterator;
use std::str::FromStr;

use daachorse::{CharwiseDoubleArrayAhoCorasick, CharwiseDoubleArrayAhoCorasickBuilder, MatchKind};

use crate::tables::Table;
use crate::{
    pagerules::PageRules,
    rule::{Conv, ConvAction, ConvRule},
    tables::expand_table,
    utils::regex,
    variant::Variant,
};

// Ref: https://github.com/wikimedia/mediawiki/blob/7bf779524ab1fd8e1d74f79ea4840564d48eea4d/includes/language/LanguageConverter.php#L76
const NESTED_RULE_MAX_DEPTH: usize = 10;

/// A ZhConverter. See also [`ZhConverterBuilder`].
pub struct ZhConverter {
    variant: Variant,
    automaton: CharwiseDoubleArrayAhoCorasick<u32>,
    target_words: Vec<String>,
}

impl ZhConverter {
    /// Create a new converter from a automaton and a mapping.
    ///
    /// It is provided for convenience and not expected to be called directly.
    /// [`ZhConverterBuilder`] would take care of these
    /// details.
    pub fn new(
        automaton: CharwiseDoubleArrayAhoCorasick<u32>,
        target_words: Vec<String>,
    ) -> ZhConverter {
        ZhConverter {
            variant: Variant::Zh,
            automaton,
            target_words: target_words,
        }
    }

    /// Create a new converter from a automaton and a mapping, as well as specifying a target
    /// variant to be used by [`convert_allowing_inline_rules`](Self::convert_allowing_inline_rules).
    pub fn with_target_variant(
        automaton: CharwiseDoubleArrayAhoCorasick<u32>,
        target_words: Vec<String>,
        variant: Variant,
    ) -> ZhConverter {
        ZhConverter {
            variant,
            automaton,
            target_words: target_words,
        }
    }

    /// Create a new converter of a sequence of `(from, to)` pairs.
    ///
    /// It use [`ZhConverterBuilder`] internally.
    #[inline(always)]
    pub fn from_pairs(pairs: &[(impl AsRef<str>, impl AsRef<str>)]) -> ZhConverter {
        let mut builder = ZhConverterBuilder::new();
        for (from, to) in pairs {
            builder = builder.add_conv_pair(from, to);
        }
        builder.build()
    }

    /// Convert a text.
    #[inline(always)]
    pub fn convert(&self, text: &str) -> String {
        let mut output = String::with_capacity(text.len());
        self.convert_to(text, &mut output);
        output
    }

    /// Same as `convert`, except that it takes a `&mut String` as dest instead of returning a `String`.
    pub fn convert_to(&self, text: &str, output: &mut String) {
        // Ref: https://github.dev/rust-lang/regex/blob/5197f21287344d2994f9cf06758a3ea30f5a26c3/src/re_trait.rs#L192
        let mut last = 0;
        // let mut cnt = HashMap::<usize, usize>::new();
        // leftmost-longest matching
        for (s, e, ti) in self
            .automaton
            .leftmost_find_iter(text)
            .map(|m| (m.start(), m.end(), m.value()))
        {
            if s > last {
                output.push_str(&text[last..s]);
            }
            // *cnt.entry(text[s..e].chars().count()).or_insert(0) += 1;
            output.push_str(&self.target_words[ti as usize]);
            last = e;
        }
        output.push_str(&text[last..]);
    }

    /// Convert a text, a long with a secondary conversion table (typically temporary).
    ///
    /// The worst-case time complexity of the implementation is `O(n*m)` where `n` and `m` are the
    /// length of the text and the maximum lengths of sources words in the secondary table
    /// (i.e. brute-force).
    fn convert_to_with(
        &self,
        mut text: &str,
        output: &mut String,
        shadowing_automaton: &CharwiseDoubleArrayAhoCorasick<u32>,
        shadowing_target_words: &[String],
        shadowed_source_words: &HashSet<String>,
    ) {
        // let mut cnt = HashMap::<usize, usize>::new();
        while !text.is_empty() {
            // leftmost-longest matching
            let (s, e, target_word) = match (
                self.automaton.leftmost_find_iter(text).next(),
                shadowing_automaton.leftmost_find_iter(text).next(),
            ) {
                (Some(a), Some(b)) if (a.start(), a.end()).cmp(&(b.start(), b.end())).is_le() => (
                    b.start(),
                    b.end(),
                    shadowing_target_words[b.value() as usize].as_ref(),
                ), // shadowed: pick a word in shadowing automaton
                (None, Some(b)) => (
                    b.start(),
                    b.end(),
                    shadowing_target_words[b.value() as usize].as_ref(),
                ), // ditto
                (Some(a), _) => {
                    // not shadowed: pick a word in original automaton
                    // check if the source word is disabled
                    if shadowed_source_words
                        .contains(self.target_words[a.value() as usize].as_str())
                    {
                        // degraded: skip one char and re-search
                        let first_char_len = text.chars().next().unwrap().len_utf8();
                        (0, first_char_len, &text[..first_char_len])
                    } else {
                        (
                            a.start(),
                            a.end(),
                            self.target_words[a.value() as usize].as_str(),
                        )
                    }
                }
                (None, None) => {
                    // end
                    output.push_str(text);
                    break;
                }
            };
            if s > 0 {
                output.push_str(&text[..s]);
            }
            // *cnt.entry(text[s..e].chars().count()).or_insert(0) += 1;
            output.push_str(target_word);
            text = &text[e..];
        }
    }

    /// Convert the given text, parsing and applying adhoc Mediawiki conversion rules in it.
    ///
    /// Basic MediaWiki conversion rules like `-{FOOBAR}-` or `-{zh-hant:FOO;zh-hans:BAR}-` are
    /// supported.
    ///
    /// Unlike [`convert_to_as_wikitext_extended`](Self::convert_to_as_wikitext_extended), rules
    /// with additional flags like `{H|zh-hant:FOO;zh-hans:BAR}` that sets global rules are simply
    /// ignored. And, it does not try to skip HTML code blocks like `<code></code>` and
    /// `<script></script>`.
    #[inline(always)]
    pub fn convert_as_wikitext_basic(&self, text: &str) -> String {
        let mut output = String::with_capacity(text.len());
        self.convert_to_as_wikitext_basic(text, &mut output);
        output
    }

    /// Convert the given text, parsing and applying adhoc and global MediaWiki conversion rules in
    /// it.
    ///
    /// Unlike [`convert_to_as_wikitext_basic`](Self::convert_to_as_wikitext_basic), all flags
    /// documented in [Help:高级字词转换语法](https://zh.wikipedia.org/wiki/Help:高级字词转换语法)
    /// are supported. And it tries to skip HTML code blocks such as `<code></code>` and
    /// `<script></script>`.
    ///
    /// # Limitations
    ///
    /// The internal implementation are intendedly replicating the behavior of
    /// [LanguageConverter.php](https://github.com/wikimedia/mediawiki/blob/7bf779524ab1fd8e1d74f79ea4840564d48eea4d/includes/language/LanguageConverter.php#L855)
    /// in MediaWiki. But it is not fully compliant with MediaWiki and providing NO PROTECTION over
    /// XSS attacks.
    ///
    /// Compared to the plain `convert`, this is known to be MUCH SLOWER due to the inevitable
    /// nature of the implementation decision made by MediaWiki.
    #[inline(always)]
    pub fn convert_as_wikitext_extended(&self, text: &str) -> String {
        let mut output = String::with_capacity(text.len());
        self.convert_to_as_wikitext_extended(text, &mut output);
        output
    }

    /// Same as [`convert_to_as_wikitext_basic`](Self::convert_to_as_wikitext_basic), except that
    /// it takes a `&mut String` as dest
    /// instead of returning a `String`.
    #[inline(always)]
    pub fn convert_to_as_wikitext_basic(&self, text: &str, output: &mut String) {
        self.convert_to_as_wikitext(text, output, false, false)
    }

    /// Same as [`convert_to_as_wikitext_extended`](Self::convert_to_as_wikitext_extended), except
    /// that it takes a `&mut String` as dest instead of returning a `String`.
    #[inline(always)]
    pub fn convert_to_as_wikitext_extended(&self, text: &str, output: &mut String) {
        self.convert_to_as_wikitext(text, output, true, true)
    }

    /// The general implementation of MediaWiki syntax-aware conversion.
    ///
    /// Equivalent to [`convert_as_wikitext_basic`](Self::convert_as_wikitext_basic) if both
    /// `skip_html_code_blocks` and `apply_global_rules` are  set to `false`.
    ///
    /// Equivalent to [`convert_as_wikitext_extended`], otherwise.
    pub fn convert_to_as_wikitext(
        &self,
        text: &str,
        output: &mut String,
        skip_html_code_blocks: bool,
        apply_global_rules: bool,
    ) {
        // Ref: https://github.com/wikimedia/mediawiki/blob/7bf779524ab1fd8e1d74f79ea4840564d48eea4d/includes/language/LanguageConverter.php#L855
        //  and https://github.com/wikimedia/mediawiki/blob/7bf779524ab1fd8e1d74f79ea4840564d48eea4d/includes/language/LanguageConverter.php#L910
        //  and https://github.com/wikimedia/mediawiki/blob/7bf779524ab1fd8e1d74f79ea4840564d48eea4d/includes/language/LanguageConverter.php#L532

        let mut convert_to: Box<dyn Fn(&str, &mut String)> =
            Box::new(|text: &str, output: &mut String| self.convert_to(text, output));
        if apply_global_rules {
            // build a secondary automaton from global rules specified in wikitext
            let mut shadowing_pairs = HashMap::new();
            let mut shadowed_source_words = HashSet::new();
            let global_rules_in_page = PageRules::from_str(text).expect("infaillible");
            for ca in global_rules_in_page.as_conv_actions() {
                match ca.adds() {
                    true => shadowing_pairs.extend(
                        ca.as_conv()
                            .get_conv_pairs(self.variant)
                            .into_iter()
                            .filter(|(f, _t)| !f.is_empty())
                            .map(|(f, t)| (f.to_owned(), t.to_owned())),
                    ),
                    false => shadowed_source_words.extend(
                        ca.as_conv()
                            .get_conv_pairs(self.variant)
                            .into_iter()
                            .map(|(f, _t)| f.to_owned()),
                    ),
                }
            }
            if !shadowing_pairs.is_empty() {
                let mut shadowing_target_words = Vec::with_capacity(shadowing_pairs.len());
                let shadowing_automaton = CharwiseDoubleArrayAhoCorasickBuilder::new()
                    .match_kind(MatchKind::LeftmostLongest)
                    .build::<_, _, u32>(shadowing_pairs.into_iter().map(|(f, t)| {
                        shadowing_target_words.push(t);
                        f
                    }))
                    .expect("Rules feed to temporay DAAC already filtered");
                convert_to = Box::new(move |text: &str, output: &mut String| {
                    self.convert_to_with(
                        text,
                        output,
                        &shadowing_automaton,
                        shadowing_target_words.as_slice(),
                        &shadowed_source_words,
                    )
                })
            }
        };

        // TODO: this may degrade to O(n^2)
        // start of rule | noHtml | noStyle | no code | no pre
        let sor_or_html = regex!(
            r#"-\{|<script.*?>.*?</script>|<style.*?>.*?</style>|<code>.*?</code>|<pre.*?>.*?</pre>"#
        );
        // start of rule
        let sor = regex!(r#"-\{"#);
        let pat_outer = if skip_html_code_blocks {
            sor_or_html
        } else {
            sor
        };
        // TODO: we need to understand what the hell it is so that to adapt it to compatible syntax
        // 		$noHtml = '<(?:[^>=]*+(?>[^>=]*+=\s*+(?:"[^"]*"|\'[^\']*\'|[^\'">\s]*+))*+[^>=]*+>|.*+)(*SKIP)(*FAIL)';
        let pat_inner = regex!(r#"-\{|\}-"#);

        let mut pos = 0;
        let mut pieces = vec![];
        while let Some(m1) = pat_outer.find_at(text, pos) {
            // convert anything before (possible) the toplevel -{
            convert_to(&text[pos..m1.start()], output);
            if m1.as_str() != "-{" {
                // not start of rule, just <foobar></foobar> to exclude
                output.push_str(&text[m1.start()..m1.end()]); // kept as-is
                pos = m1.end();
                continue; // i.e. <SKIP><FAIL>
            }
            // found toplevel -{
            pos = m1.start() + 2;
            pieces.push(String::new());
            while let Some(m2) = pat_inner.find_at(text, pos) {
                // let mut piece = String::from(&text[pos..m2.start()]);
                if m2.as_str() == "-{" {
                    // if there are two many open start tag, ignore the new nested rule
                    if pieces.len() >= NESTED_RULE_MAX_DEPTH {
                        pos += 2;
                        continue;
                    }
                    // start tag
                    pieces.last_mut().unwrap().push_str(&text[pos..m2.start()]);
                    pieces.push(String::new()); // e.g. -{ zh: AAA -{
                    pos = m2.end();
                } else {
                    // end tag
                    let mut piece = pieces.pop().unwrap();
                    piece.push_str(&text[pos..m2.start()]);
                    // only take it output; mutations to global rules are ignored
                    let r = ConvRule::from_str_infallible(&piece);
                    if let Some(upper) = pieces.last_mut() {
                        write!(upper, "{}", r.targeted(self.variant)).unwrap();
                    } else {
                        write!(output, "{}", r.targeted(self.variant)).unwrap();
                    };
                    // if let Ok(rule) = dbg!(ConvRule::from_str(&piece)) {
                    //     rule.write_output(upper, self.variant).unwrap();
                    // } else {
                    //     // rule is invalid
                    //     // TODO: what should we do actually? for now, we just do nothing to it
                    //     upper.push_str(&piece);
                    // }
                    pos = m2.end();
                    if pieces.is_empty() {
                        // return to toplevel
                        break;
                    }
                }
            }
            while let Some(piece) = pieces.pop() {
                output.push_str("-{");
                output.push_str(&piece);
            }
            // TODO: produce convert(&text[pos..])
        }
        if pos < text.len() {
            // no more conv rules, just convert and append
            output.push_str(&self.convert(&text[pos..]));
        }
    }

    // #[inline(always)]
    // pub fn convert_applying_mediawiki_rules(
    //     &self,
    //     text: &str,
    //     applying_global_rules: bool,
    // ) -> String {
    //     let mut output = String::with_capacity(text.len());
    //     self.convert_to_applying_mediawiki_rules(text, &mut output, applying_global_rules);
    //     output
    // }

    // TODO: inplace? we need to maintain a stack which could be at most O(n)
    //       and it requires access to underlying bytes for subtle mutations
    // pub fn convert_inplace(&self, text: &mut String) {
    //     let tbp = VecDeque::<&str>::new(); // to be pushed
    //     let mut wi = 0; // writing index
    //     let mut ri = 0; // reading index
    //     while let Some((s, e)) = self.regex.find_at(text, ri).map(|m| (m.start(), m.end())) {
    //         while !tbp.is_empty() && s - wi >= tbp[0].len() {
    //             let raw = unsafe { text.as_bytes_mut() };
    //             raw[wi..wi + tbp[0].len()].clone_from_slice(tbp[0].as_bytes());
    //             tbp.pop_front();
    //         }
    //     }
    // }

    /// Count the sum of lengths of matched source words to be substituted in the given text.
    pub fn count_matched(&self, text: &str) -> usize {
        self.automaton
            .leftmost_find_iter(text)
            .map(|m| m.end() - m.start())
            .sum()
    }
}

/// A builder that helps build a `ZhConverter`.
///
/// # Example
/// Build a Zh2CN converter with some additional rules.
/// ```
/// use zhconv::{zhconv, ZhConverterBuilder, Variant, tables::ZH_HANS_CN_TABLE};
/// // extracted from https://zh.wikipedia.org/wiki/Template:CGroup/Template:CGroup/文學.
/// let conv_lines = r"zh-hans:三个火枪手;zh-hant:三劍客;zh-tw:三劍客;
///                    zh-cn:雾都孤儿;zh-tw:孤雛淚;zh-hk:苦海孤雛;zh-sg:雾都孤儿;zh-mo:苦海孤雛;";
/// let converter = ZhConverterBuilder::new()
///                     .target(Variant::ZhCN)
///                     .table(*ZH_HANS_CN_TABLE)
///                     .dfa(true) // dfa enabled: slower build, faster conversion
///                     .conv_lines(conv_lines)
///                     .build();
/// let original = "《三劍客》是亞歷山大·仲馬的作品。《孤雛淚》是查爾斯·狄更斯的作品。";
/// assert_eq!(converter.convert(original), "《三个火枪手》是亚历山大·仲马的作品。《雾都孤儿》是查尔斯·狄更斯的作品。");
/// assert_eq!(zhconv(original, Variant::ZhCN), "《三剑客》是亚历山大·仲马的作品。《孤雏泪》是查尔斯·狄更斯的作品。")
#[derive(Debug, Clone, Default)]
pub struct ZhConverterBuilder<'t> {
    target: Variant,
    /// The base conversion table
    tables: Vec<(&'t str, &'t str)>,
    /// Rules to be added, from page rules or cgroups
    adds: HashMap<String, String>,
    /// Rules to be removed, from page rules or cgroups
    removes: HashMap<String, String>, // TODO: unnecessary owned type
}

impl<'t> ZhConverterBuilder<'t> {
    pub fn new() -> Self {
        Default::default()
    }

    /// Set the target Chinese variant to convert to.
    ///
    /// The target variant is only useful to get proper conv pairs from
    /// [`ConvRule`](crate::rule::ConvRule)s. That is, if only tables are specified, the target
    /// variant would be useless.
    pub fn target(mut self, variant: Variant) -> Self {
        self.target = variant;
        self
    }

    /// Add a conversion table, which is typically those in [`tables`](crate::tables).
    pub fn table(mut self, table: Table<'t>) -> Self {
        self.tables.push(table);
        self
    }

    /// Add a set of conversion tables, which are typically returned by [`get_builtin_converter`](crate::get_builtin_converter).
    pub fn tables(mut self, tables: &[Table<'t>]) -> Self {
        self.tables.extend(tables.iter());
        self
    }

    //  [CGroup](https://zh.wikipedia.org/wiki/Module:CGroup) (a.k.a 公共轉換組)

    /// Add a set of rules extracted from a page.
    ///
    /// This is a helper wrapper around `page_rules`.
    #[inline(always)]
    pub fn rules_from_page(self, text: &str) -> Self {
        self.page_rules(
            &PageRules::from_str(text).expect("Page rules parsing is infallible for now"),
        )
    }

    /// Add a set of rules from `PageRules`.
    #[inline(always)]
    pub fn page_rules(self, page_rules: &PageRules) -> Self {
        self.conv_actions(page_rules.as_conv_actions())
    }

    /// Add a set of rules.
    ///
    /// These rules take the higher precedence over those specified via `table`.
    fn conv_actions<'i>(mut self, conv_actions: impl IntoIterator<Item = &'i ConvAction>) -> Self {
        for conv_action in conv_actions {
            let pairs = conv_action.as_conv().get_conv_pairs(self.target);
            if conv_action.adds() {
                self.adds
                    .extend(pairs.iter().map(|&(f, t)| (f.to_owned(), t.to_owned())));
            } else {
                self.removes
                    .extend(pairs.iter().map(|&(f, t)| (f.to_owned(), t.to_owned())));
            }
        }
        self
    }

    /// Add a [`Conv`].
    ///
    /// For general cases, check [`add_conv_pair`](#method.add_conv_pair) which takes a plain
    /// `from -> to` pair.
    pub fn add_conv(mut self, conv: Conv) -> Self {
        let pairs = conv.get_conv_pairs(self.target);
        self.adds
            .extend(pairs.iter().map(|&(f, t)| (f.to_owned(), t.to_owned())));
        self
    }

    /// Mark a conv as removed.
    pub fn remove_conv(mut self, conv: Conv) -> Self {
        let pairs = conv.get_conv_pairs(self.target);
        self.removes
            .extend(pairs.iter().map(|&(f, t)| (f.to_owned(), t.to_owned())));
        self
    }

    /// Add a single `from -> to` conversion pair.
    ///
    /// It takes the precedence over those specified via `table`. It shares the same precedence level with those specified via `cgroup`.
    pub fn add_conv_pair(mut self, from: impl AsRef<str>, to: impl AsRef<str>) -> Self {
        let (from, to): (&str, &str) = (from.as_ref(), to.as_ref());
        if from.is_empty() {
            panic!("Conv pair should have non-empty from.")
        }
        self.adds.insert(from.to_owned(), to.to_owned());
        self
    }

    /// Mark a single conversion pair as removed.
    ///
    /// Any rule with the same `from`, whether specified via `add_conv_pair`, `conv_lines` or `table`, is removed.
    pub fn remove_conv_pair(mut self, from: impl AsRef<str>, to: impl AsRef<str>) -> Self {
        self.removes
            .insert(from.as_ref().to_owned(), to.as_ref().to_owned());
        self
    }

    /// Add a text of conv lines.
    ///
    /// e.g.
    /// ```text
    /// zh-cn:天堂执法者; zh-hk:夏威夷探案; zh-tw:檀島警騎2.0;
    /// zh-cn:史蒂芬·'史蒂夫'·麦格瑞特; zh-tw:史提夫·麥加雷; zh-hk:麥星帆;
    /// zh-cn:丹尼尔·'丹尼/丹诺'·威廉姆斯; zh-tw:丹尼·威廉斯; zh-hk:韋丹尼;
    /// ```
    pub fn conv_lines(mut self, lines: &str) -> Self {
        for line in lines.lines().map(str::trim).filter(|s| !s.is_empty()) {
            if let Ok(conv) = Conv::from_str(line.trim()) {
                self.adds
                    .extend(conv.get_conv_pairs(self.target).iter().map(|&(f, t)| {
                        if f.is_empty() {
                            panic!("Conv pair should have non-empty from.")
                        }
                        (f.to_owned(), t.to_owned())
                    }));
            }
        }
        self
    }

    // /// Set whether to activate the feature DFA of Aho-Corasick.
    // ///
    // /// With DFA enabled, it takes rougly 5x time to build the converter while the conversion
    // /// speed is < 2x faster. All built-in converters have this feature enabled for better
    // /// conversion performance. In other cases with this flag unset, the implementation would
    // /// determine by itself whether to enable it per the number of patterns.
    // pub fn dfa(mut self, enabled: bool) -> Self {
    //     self.dfa = enabled;
    //     self
    // }

    /// Do the build.
    ///
    /// It internally aggregate previously specified tables, rules and pairs, from where an
    /// automaton and a mapping are built, which are then feed into the new converter.
    pub fn build(&self) -> ZhConverter {
        let Self {
            target,
            tables,
            adds,
            removes,
        } = self;
        // let v = lz4_flex::compress_prepend_size(b"hello")
        // dbg!(v.len());
        // TODO: do we need a HashMap at all?
        let mut mapping = HashMap::with_capacity(
            (tables.iter().map(|(fs, _ts)| fs.len()).sum::<usize>() + adds.len())
                .saturating_sub(removes.len()),
        );
        mapping.extend(
            tables
                .iter()
                .flat_map(|&table| expand_table(table))
                .filter(|(from, to)| !(from.is_empty() && to.is_empty())) // empty str would trouble AC
                .filter(|(from, _to)| !removes.contains_key(from)),
        );
        mapping.extend(
            adds.iter()
                .filter(|(from, _to)| !removes.contains_key(from.as_str()))
                .map(|(from, to)| (from.to_owned(), to.to_owned())),
        );
        let sequence = mapping.keys();
        let automaton = CharwiseDoubleArrayAhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .build(sequence)
            .unwrap();
        ZhConverter {
            variant: *target,
            automaton,
            target_words: mapping.into_values().collect(),
        }
    }
}
