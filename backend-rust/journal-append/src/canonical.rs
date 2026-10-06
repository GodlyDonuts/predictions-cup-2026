use std::collections::HashSet;

// Admission for the compact bytes emitted by JSON.stringify. Checking only a
// raw-body hash would accept duplicate keys or alternate numeric encodings that
// the original JS Journal parses and reserializes differently.
#[cfg(test)]
pub fn check(raw: &str) -> Result<(), String> {
    let mut p = Parser {
        bytes: raw.as_bytes(),
        at: 0,
    };
    p.value(0)?;
    if p.at != p.bytes.len() {
        return Err("JOURNAL_CANONICAL_REQUIRED".into());
    }
    Ok(())
}
// Scan one value inside the fixed journal envelope without reparsing or
// allocating the entire historical state with serde.
pub fn end(raw: &str, start: usize) -> Result<usize, String> {
    let mut p = Parser {
        bytes: raw.as_bytes(),
        at: start,
    };
    p.value(0)?;
    Ok(p.at)
}
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Parser<'a> {
    fn need(&self, ok: bool) -> Result<(), String> {
        if ok {
            Ok(())
        } else {
            Err("JOURNAL_CANONICAL_REQUIRED".into())
        }
    }
    fn byte(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
    fn value(&mut self, depth: usize) -> Result<(), String> {
        self.need(depth <= 256)?;
        match self.byte() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => {
                self.string()?;
                Ok(())
            }
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err("JOURNAL_CANONICAL_REQUIRED".into()),
        }
    }
    fn literal(&mut self, s: &[u8]) -> Result<(), String> {
        self.need(self.bytes.get(self.at..self.at + s.len()) == Some(s))?;
        self.at += s.len();
        Ok(())
    }
    fn number(&mut self) -> Result<(), String> {
        let start = self.at;
        while self
            .byte()
            .is_some_and(|b| b.is_ascii_digit() || b"-+.eE".contains(&b))
        {
            self.at += 1;
        }
        let raw = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| "JOURNAL_CANONICAL_REQUIRED")?;
        // Exact integers within JS's safe range need no float parse/format round trip.
        let digits = raw.strip_prefix('-').unwrap_or(raw);
        if !digits.is_empty()
            && (digits.len() == 1 || !digits.starts_with('0'))
            && digits.bytes().all(|b| b.is_ascii_digit())
        {
            if let Ok(n) = digits.parse::<u64>() {
                if n <= 9007199254740991 && (n != 0 || !raw.starts_with('-')) {
                    return Ok(());
                }
            }
        }
        let n = raw
            .parse::<f64>()
            .map_err(|_| "JOURNAL_CANONICAL_REQUIRED")?;
        self.need(n.is_finite())?;
        let mut buffer = ryu_js::Buffer::new();
        self.need(buffer.format_finite(n) == raw)
    }
    fn hex(&self, start: usize) -> Result<u16, String> {
        let s = self
            .bytes
            .get(start..start + 4)
            .ok_or("JOURNAL_CANONICAL_REQUIRED")?;
        self.need(
            s.iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b)),
        )?;
        u16::from_str_radix(
            std::str::from_utf8(s).map_err(|_| "JOURNAL_CANONICAL_REQUIRED")?,
            16,
        )
        .map_err(|_| "JOURNAL_CANONICAL_REQUIRED".into())
    }
    fn string(&mut self) -> Result<&'a [u8], String> {
        let start = self.at;
        self.need(self.byte() == Some(b'"'))?;
        self.at += 1;
        loop {
            match self.byte() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(&self.bytes[start..self.at]);
                }
                Some(b'\\') => {
                    self.at += 1;
                    match self.byte() {
                        Some(b'"' | b'\\' | b'b' | b'f' | b'n' | b'r' | b't') => self.at += 1,
                        Some(b'u') => {
                            let n = self.hex(self.at + 1)?;
                            self.need(
                                (n < 32 && ![8, 9, 10, 12, 13].contains(&n))
                                    || (0xd800..=0xdfff).contains(&n),
                            )?;
                            self.at += 5;
                            if (0xd800..=0xdbff).contains(&n)
                                && self.bytes.get(self.at..self.at + 2) == Some(b"\\u")
                            {
                                let next = self.hex(self.at + 2)?;
                                self.need(!(0xdc00..=0xdfff).contains(&next))?;
                            }
                        }
                        _ => return Err("JOURNAL_CANONICAL_REQUIRED".into()),
                    }
                }
                Some(b) if b >= 32 => self.at += 1,
                _ => return Err("JOURNAL_CANONICAL_REQUIRED".into()),
            }
        }
    }
    fn array(&mut self, depth: usize) -> Result<(), String> {
        self.at += 1;
        if self.byte() == Some(b']') {
            self.at += 1;
            return Ok(());
        }
        loop {
            self.value(depth)?;
            match self.byte() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(());
                }
                _ => return Err("JOURNAL_CANONICAL_REQUIRED".into()),
            }
        }
    }
    fn object(&mut self, depth: usize) -> Result<(), String> {
        self.at += 1;
        if self.byte() == Some(b'}') {
            self.at += 1;
            return Ok(());
        }
        // Most recorded objects have a handful of fields. Avoid a per-object hash
        // table allocation there; promote large objects to keep bounded complexity.
        let mut short_keys = Vec::with_capacity(12);
        let mut keys = None;
        let mut previous_index = None;
        let mut ordinary = false;
        loop {
            let key = self.string()?;
            if let Some(set) = keys.as_mut() {
                self.need(HashSet::<&[u8]>::insert(set, key))?;
            } else {
                self.need(!short_keys.contains(&key))?;
                short_keys.push(key);
                if short_keys.len() == 12 {
                    keys = Some(short_keys.iter().copied().collect::<HashSet<_>>());
                }
            }
            let digits = &key[1..key.len() - 1];
            let index = if !digits.is_empty()
                && (digits.len() == 1 || digits[0] != b'0')
                && digits.iter().all(u8::is_ascii_digit)
            {
                std::str::from_utf8(digits)
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|n| *n < 4294967295)
            } else {
                None
            };
            if let Some(n) = index {
                self.need(!ordinary && previous_index.is_none_or(|previous| n > previous))?;
                previous_index = Some(n);
            } else {
                ordinary = true;
            }
            self.need(self.byte() == Some(b':'))?;
            self.at += 1;
            self.value(depth)?;
            match self.byte() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(());
                }
                _ => return Err("JOURNAL_CANONICAL_REQUIRED".into()),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::check;
    #[test]
    fn canonical_numbers_strings_and_keys() {
        for s in [
            r#"{"0":0,"2":2,"a":[0.005,1e-7,1e+21,-1e-7],"b":"a\n\u0000💥\ud800x\udfff"}"#,
            r#"{"4294967295":true,"01":null,"x":[]}"#,
        ] {
            assert!(check(s).is_ok(), "{s}");
        }
    }
    #[test]
    fn reserialization_differences_are_rejected() {
        for s in [
            "-0",
            "1.0",
            "1e-07",
            "1e21",
            "01",
            "1e400",
            r#""\/""#,
            r#""\u0061""#,
            r#""\u000a""#,
            r#""\uD800""#,
            r#""\ud83d\udca5""#,
            r#"{"x":1,"x":2}"#,
            r#"{"2":2,"0":0}"#,
            r#"{"a":1,"0":0}"#,
            r#"[1,]"#,
            r#"{"x": 1}"#,
        ] {
            assert!(check(s).is_err(), "{s}");
        }
    }
}
