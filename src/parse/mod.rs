#![allow(non_snake_case)]

use std::{borrow::Cow, str::from_utf8};

use nom::{
    branch::alt,
    bytes::streaming::{tag, take_while, take_while1, take_while_m_n},
    character::streaming::digit1,
    character::{is_alphabetic, is_digit},
    combinator::{map, map_res, opt, recognize},
    multi::{many0, separated_list1},
    sequence::{delimited, tuple},
    IResult,
};

use crate::types::AtomOrQuoted;

pub mod address;
pub mod command;
pub mod response;

pub fn base64(input: &[u8]) -> IResult<&[u8], &str> {
    let mut parser = map_res(
        recognize(tuple((
            take_while(is_base64_char),
            opt(alt((tag("=="), tag("=")))),
        ))),
        from_utf8,
    );

    let (remaining, base64) = parser(input)?;

    Ok((remaining, base64))
}

fn is_base64_char(i: u8) -> bool {
    is_alphabetic(i) || is_digit(i) || i == b'+' || i == b'/'
}

pub fn number(input: &[u8]) -> IResult<&[u8], u32> {
    map_res(map_res(digit1, from_utf8), str::parse::<u32>)(input) // FIXME(perf): use from_utf8_unchecked
}

// -------------------------------------------------------------------------------------------------

/// String = Atom / Quoted-string
pub fn String(input: &[u8]) -> IResult<&[u8], AtomOrQuoted> {
    alt((
        map(Atom, |atom| AtomOrQuoted::Atom(atom.into())),
        map(Quoted_string, |quoted| AtomOrQuoted::Quoted(quoted.into())),
    ))(input)
}

/// Atom = 1*atext
pub fn Atom(input: &[u8]) -> IResult<&[u8], &str> {
    map_res(take_while1(is_atext), std::str::from_utf8)(input)
}

/// Printable US-ASCII characters not including specials.
/// Used for atoms.
///
/// atext = ALPHA / DIGIT /
///          "!" / "#" /
///          "$" / "%" /
///          "&" / "'" /
///          "*" / "+" /
///          "-" / "/" /
///          "=" / "?" /
///          "^" / "_" /
///          "`" / "{" /
///          "|" / "}" /
///          "~"
pub fn is_atext(byte: u8) -> bool {
    let allowed = b"!#$%&'*+-/=?^_`{|}~";

    is_alphabetic(byte) || is_digit(byte) || allowed.contains(&byte)
}

/// Quoted-string = DQUOTE *QcontentSMTP DQUOTE
pub fn Quoted_string(input: &[u8]) -> IResult<&[u8], Cow<'_, str>> {
    map(
        delimited(
            tag("\""),
            map_res(recognize(many0(QcontentSMTP)), std::str::from_utf8),
            tag("\""),
        ),
        unescape_quoted,
    )(input)
}

/// QcontentSMTP = qtextSMTP / quoted-pairSMTP
pub fn QcontentSMTP(input: &[u8]) -> IResult<&[u8], &[u8]> {
    let parser = alt((take_while_m_n(1, 1, is_qtextSMTP), quoted_pairSMTP));

    let (remaining, parsed) = recognize(parser)(input)?;

    Ok((remaining, parsed))
}

/// Within a quoted string, any ASCII graphic or space is permitted
/// without blackslash-quoting except double-quote and the backslash itself.
///
/// qtextSMTP = %d32-33 / %d35-91 / %d93-126
pub fn is_qtextSMTP(byte: u8) -> bool {
    matches!(byte, 32..=33 | 35..=91 | 93..=126)
}

/// Backslash followed by any ASCII graphic (including itself) or SPace
///
/// quoted-pairSMTP = %d92 %d32-126
///
/// FIXME: How should e.g. "\a" be interpreted?
pub fn quoted_pairSMTP(input: &[u8]) -> IResult<&[u8], &[u8]> {
    //fn is_value(byte: u8) -> bool {
    //    matches!(byte, 32..=126)
    //}

    // FIXME: Only allow "\\" and "\"" for now ...
    fn is_value(byte: u8) -> bool {
        byte == b'\\' || byte == b'\"'
    }

    let parser = tuple((tag("\\"), take_while_m_n(1, 1, is_value)));

    let (remaining, parsed) = recognize(parser)(input)?;

    Ok((remaining, parsed))
}

// -------------------------------------------------------------------------------------------------

/// Domain = sub-domain *("." sub-domain)
pub fn Domain(input: &[u8]) -> IResult<&[u8], &str> {
    let parser = separated_list1(tag(b"."), sub_domain);

    let (remaining, parsed) = map_res(recognize(parser), std::str::from_utf8)(input)?;

    Ok((remaining, parsed))
}

/// sub-domain = Let-dig [Ldh-str]
pub fn sub_domain(input: &[u8]) -> IResult<&[u8], &[u8]> {
    let parser = tuple((take_while_m_n(1, 1, is_Let_dig), opt(Ldh_str)));

    let (remaining, parsed) = recognize(parser)(input)?;

    Ok((remaining, parsed))
}

/// Let-dig = ALPHA / DIGIT
pub fn is_Let_dig(byte: u8) -> bool {
    is_alphabetic(byte) || is_digit(byte)
}

/// Ldh-str = *( ALPHA / DIGIT / "-" ) Let-dig
pub fn Ldh_str(input: &[u8]) -> IResult<&[u8], &[u8]> {
    let parser = many0(alt((
        take_while_m_n(1, 1, is_alphabetic),
        take_while_m_n(1, 1, is_digit),
        recognize(tuple((tag(b"-"), take_while_m_n(1, 1, is_Let_dig)))),
    )));

    let (remaining, parsed) = recognize(parser)(input)?;

    Ok((remaining, parsed))
}

// -------------------------------------------------------------------------------------------------

pub(crate) fn escape_quoted(unescaped: &str) -> Cow<str> {
    let mut escaped = Cow::Borrowed(unescaped);

    if escaped.contains('\\') {
        escaped = Cow::Owned(escaped.replace('\\', "\\\\"));
    }

    if escaped.contains('\"') {
        escaped = Cow::Owned(escaped.replace('\"', "\\\""));
    }

    escaped
}

pub(crate) fn unescape_quoted(escaped: &str) -> Cow<str> {
    let mut unescaped = Cow::Borrowed(escaped);

    if unescaped.contains("\\\\") {
        unescaped = Cow::Owned(unescaped.replace("\\\\", "\\"));
    }

    if unescaped.contains("\\\"") {
        unescaped = Cow::Owned(unescaped.replace("\\\"", "\""));
    }

    unescaped
}

#[cfg(test)]
pub mod test {
    use super::sub_domain;

    #[test]
    fn test_subdomain() {
        let (rem, parsed) = sub_domain(b"example???").unwrap();
        assert_eq!(parsed, b"example");
        assert_eq!(rem, b"???");
    }
}
