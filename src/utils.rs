macro_rules! get_with_fallback {
    ( $map:expr, $key:expr, $( $others:tt )* ) => {
        $map.get(&$key).or_else(|| get_with_fallback!($map, $($others)* ))
    };
    ( $map:expr, $key:expr ) => {
        $map.get(&$key)
    };
}
pub(crate) use get_with_fallback;

macro_rules! for_wasm {
    ($($item:item)*) => {$(
        #[cfg(target_arch = "wasm32")]
        $item
    )*}
}
pub(crate) use for_wasm;

pub fn zstd_decompress(bytes: &[u8]) -> Vec<u8> {
    use std::io::Read;

    let mut buf = vec![];
    ruzstd::StreamingDecoder::new(bytes)
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    buf
}

macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}
pub(crate) use regex;
