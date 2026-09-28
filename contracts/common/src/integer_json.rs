//! Soroban's bounded JSON input parser. Construct the SDK's ordinary JSON Value
//! without linking serde's generic floating-point deserializer into Wasm.
//! The caller still requires exact canonical re-encoding before authentication.
use crate::Error;
use alloc::{string::String, vec::Vec};
use serde_json::{Map, Value};

pub fn parse(text: &str) -> Result<Value, Error> {
    if text.len() > 1024 {
        return Err(Error::InvalidEvidence);
    }
    let mut p = Parser {
        input: text.as_bytes(),
        at: 0,
        entries: 0,
    };
    let value = p.value(0)?;
    if p.at != p.input.len() {
        return Err(Error::InvalidEvidence);
    }
    Ok(value)
}

struct Parser<'a> {
    input: &'a [u8],
    at: usize,
    entries: usize,
}
impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.at).copied()
    }
    fn byte(&mut self) -> Result<u8, Error> {
        let value = self.peek().ok_or(Error::InvalidEvidence)?;
        self.at += 1;
        Ok(value)
    }
    fn expect(&mut self, expected: u8) -> Result<(), Error> {
        if self.byte()? == expected {
            Ok(())
        } else {
            Err(Error::InvalidEvidence)
        }
    }
    fn literal(&mut self, expected: &[u8]) -> Result<(), Error> {
        for &b in expected {
            self.expect(b)?;
        }
        Ok(())
    }
    fn entry(&mut self) -> Result<(), Error> {
        self.entries += 1;
        if self.entries > 128 {
            Err(Error::InvalidEvidence)
        } else {
            Ok(())
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, Error> {
        if depth > 8 {
            return Err(Error::InvalidEvidence);
        }
        match self.peek().ok_or(Error::InvalidEvidence)? {
            b'{' => {
                self.at += 1;
                let mut out = Map::new();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                    return Ok(Value::Object(out));
                }
                loop {
                    self.entry()?;
                    let key = self.string()?;
                    if key.is_empty() || key.len() > 128 {
                        return Err(Error::InvalidEvidence);
                    }
                    self.expect(b':')?;
                    let value = self.value(depth + 1)?;
                    if out.insert(key, value).is_some() {
                        return Err(Error::InvalidEvidence);
                    }
                    match self.byte()? {
                        b'}' => break,
                        b',' => {}
                        _ => return Err(Error::InvalidEvidence),
                    }
                }
                Ok(Value::Object(out))
            }
            b'[' => {
                self.at += 1;
                let mut out = Vec::new();
                if self.peek() == Some(b']') {
                    self.at += 1;
                    return Ok(Value::Array(out));
                }
                loop {
                    self.entry()?;
                    out.push(self.value(depth + 1)?);
                    match self.byte()? {
                        b']' => break,
                        b',' => {}
                        _ => return Err(Error::InvalidEvidence),
                    }
                }
                Ok(Value::Array(out))
            }
            b'"' => self.string().map(Value::String),
            b't' => {
                self.literal(b"true")?;
                Ok(Value::Bool(true))
            }
            b'f' => {
                self.literal(b"false")?;
                Ok(Value::Bool(false))
            }
            b'n' => {
                self.literal(b"null")?;
                Ok(Value::Null)
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(Error::InvalidEvidence),
        }
    }
    fn number(&mut self) -> Result<Value, Error> {
        let negative = self.peek() == Some(b'-');
        if negative {
            self.at += 1;
        }
        let first = self.byte()?;
        if !first.is_ascii_digit() {
            return Err(Error::InvalidEvidence);
        }
        let mut number = u64::from(first - b'0');
        while let Some(b @ b'0'..=b'9') = self.peek() {
            if first == b'0' {
                return Err(Error::InvalidEvidence);
            }
            number = number
                .checked_mul(10)
                .and_then(|n| n.checked_add(u64::from(b - b'0')))
                .ok_or(Error::InvalidEvidence)?;
            self.at += 1;
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(Error::InvalidEvidence);
        }
        if !negative {
            return Ok(Value::Number(number.into()));
        }
        let signed = if number == (1u64 << 63) {
            i64::MIN
        } else {
            -i64::try_from(number).map_err(|_| Error::InvalidEvidence)?
        };
        Ok(Value::Number(signed.into()))
    }
    fn hex4(&mut self) -> Result<u32, Error> {
        let mut out = 0;
        for _ in 0..4 {
            let value = match self.byte()? {
                b @ b'0'..=b'9' => b - b'0',
                b @ b'a'..=b'f' => b - b'a' + 10,
                b @ b'A'..=b'F' => b - b'A' + 10,
                _ => return Err(Error::InvalidEvidence),
            };
            out = out * 16 + u32::from(value);
        }
        Ok(out)
    }
    fn string(&mut self) -> Result<String, Error> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            match self.byte()? {
                b'"' => return String::from_utf8(out).map_err(|_| Error::InvalidEvidence),
                b'\\' => match self.byte()? {
                    b @ (b'"' | b'\\' | b'/') => out.push(b),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'n' => out.push(10),
                    b'r' => out.push(13),
                    b't' => out.push(9),
                    b'u' => {
                        let mut code = self.hex4()?;
                        if (0xD800..=0xDBFF).contains(&code) {
                            self.literal(b"\\u")?;
                            let low = self.hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err(Error::InvalidEvidence);
                            }
                            code = 0x10000 + ((code - 0xD800) << 10) + low - 0xDC00;
                        }
                        let value = char::from_u32(code).ok_or(Error::InvalidEvidence)?;
                        let mut encoded = [0; 4];
                        out.extend_from_slice(value.encode_utf8(&mut encoded).as_bytes());
                    }
                    _ => return Err(Error::InvalidEvidence),
                },
                0..=31 => return Err(Error::InvalidEvidence),
                b => out.push(b),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integer_parser_matches_json_values_and_fails_closed_on_malformed_inputs() {
        for input in [
            r#"{}"#,
            r#"{"a":[null,true,false,-1,0,18446744073709551615],"b":{"c":"é🦀"}}"#,
            r#"{"x":"\u0000\b\f\n\r\t\\\"\/\uD83E\uDD80"}"#,
            r#"{"x":-9223372036854775808}"#,
        ] {
            assert_eq!(
                parse(input).unwrap(),
                serde_json::from_str::<Value>(input).unwrap()
            );
        }
        for input in [
            "",
            " ",
            "{}x",
            "{",
            "[",
            "[1,]",
            "{\"x\":}",
            "{\"x\":1,}",
            r#"{"x":0,"x":1}"#,
            r#"{"x":"\uD800"}"#,
            r#"{"x":"\uDC00"}"#,
            r#"{"x":"\uD800\u0000"}"#,
            r#"{"x":"\u000g"}"#,
            r#"{"x":"\q"}"#,
            r#"{"x":01}"#,
            r#"{"x":-}"#,
            r#"{"x":1.0}"#,
            r#"{"x":1e2}"#,
            r#"{"x":18446744073709551616}"#,
            r#"{"x":-9223372036854775809}"#,
            r#"{"":0}"#,
        ] {
            assert!(parse(input).is_err(), "accepted {input}");
        }
        let too_big = alloc::format!("{{\"x\":\"{}\"}}", "a".repeat(1025));
        assert!(parse(&too_big).is_err());
        let largest = alloc::format!("{{\"x\":[{}0]}}", "0,".repeat(126));
        assert!(parse(&largest).is_ok()); // one field plus 127 array entries
        let deep = alloc::format!("{{\"x\":{}0{}}}", "[".repeat(9), "]".repeat(9));
        assert!(parse(&deep).is_err());
        let wide = alloc::format!("{{\"x\":[{}0]}}", "0,".repeat(128));
        assert!(parse(&wide).is_err());
    }
}
