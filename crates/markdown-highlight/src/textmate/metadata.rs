//! `src/encodedTokenAttributes.ts`: a token's language, type, font style
//! and colors packed in 32 bits.

/// `OptionalStandardTokenType.NotSet`.
pub const TOKEN_TYPE_NOT_SET: u32 = 8;

pub fn language_id(metadata: u32) -> u32 {
    metadata & 0xff
}

pub fn token_type(metadata: u32) -> u32 {
    (metadata & 0x300) >> 8
}

pub fn contains_balanced_brackets(metadata: u32) -> bool {
    metadata & 0x400 != 0
}

pub fn font_style(metadata: u32) -> u32 {
    (metadata & 0x7800) >> 11
}

pub fn foreground(metadata: u32) -> u32 {
    (metadata & 0x00ff_8000) >> 15
}

pub fn background(metadata: u32) -> u32 {
    (metadata & 0xff00_0000) >> 24
}

/// `EncodedTokenMetadata.set`: 0, `NotSet` (-1 or 8) and `None` leave a
/// field as it is.
pub fn set(
    metadata: u32,
    language: u32,
    token: u32,
    balanced_brackets: Option<bool>,
    style: i32,
    fg: u32,
    bg: u32,
) -> u32 {
    let mut language_value = language_id(metadata);
    let mut token_value = token_type(metadata);
    let mut brackets = u32::from(contains_balanced_brackets(metadata));
    let mut style_value = font_style(metadata);
    let mut fg_value = foreground(metadata);
    let mut bg_value = background(metadata);
    if language != 0 {
        language_value = language;
    }
    if token != TOKEN_TYPE_NOT_SET {
        token_value = token;
    }
    if let Some(value) = balanced_brackets {
        brackets = u32::from(value);
    }
    if style != -1 {
        style_value = style as u32;
    }
    if fg != 0 {
        fg_value = fg;
    }
    if bg != 0 {
        bg_value = bg;
    }
    // As with JavaScript's 32-bit `<<`, bits shifted past 31 are lost.
    language_value
        | token_value << 8
        | brackets << 10
        | style_value << 11
        | fg_value << 15
        | bg_value << 24
}
