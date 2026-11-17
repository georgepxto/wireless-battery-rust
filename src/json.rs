//! Parser JSON minimo (so leitura), para a saida do HeadsetControl sem depender do serde.

#[derive(Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn arr(&self) -> &[Value] {
        match self {
            Value::Arr(a) => a,
            _ => &[],
        }
    }
}

pub fn parse(text: &str) -> Option<Value> {
    let mut p = Parser { s: text.as_bytes(), i: 0, depth: 0 };
    let v = p.value()?;
    p.ws();
    (p.i == p.s.len()).then_some(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: u32,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        self.ws();
        (self.s.get(self.i) == Some(&c)).then(|| self.i += 1)
    }

    fn lit(&mut self, word: &str, v: Value) -> Option<Value> {
        self.s[self.i..].starts_with(word.as_bytes()).then(|| {
            self.i += word.len();
            v
        })
    }

    fn value(&mut self) -> Option<Value> {
        self.ws();
        self.depth += 1;
        if self.depth > 64 {
            return None;
        }
        let v = match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut fields = Vec::new();
                if self.eat(b'}').is_none() {
                    loop {
                        self.ws();
                        let key = self.string()?;
                        self.eat(b':')?;
                        fields.push((key, self.value()?));
                        if self.eat(b',').is_none() {
                            self.eat(b'}')?;
                            break;
                        }
                    }
                }
                Value::Obj(fields)
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                if self.eat(b']').is_none() {
                    loop {
                        items.push(self.value()?);
                        if self.eat(b',').is_none() {
                            self.eat(b']')?;
                            break;
                        }
                    }
                }
                Value::Arr(items)
            }
            b'"' => Value::Str(self.string()?),
            b't' => self.lit("true", Value::Bool(true))?,
            b'f' => self.lit("false", Value::Bool(false))?,
            b'n' => self.lit("null", Value::Null)?,
            _ => {
                let start = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                Value::Num(std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()?)
            }
        };
        self.depth -= 1;
        Some(v)
    }

    fn string(&mut self) -> Option<String> {
        if self.s.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' | b'f' => {}
                        b'u' => {
                            let hex = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
                            self.i += 4;
                            let ch = char::from_u32(u32::from_str_radix(hex, 16).ok()?).unwrap_or('\u{FFFD}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other),
                    }
                }
                _ => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_values() {
        let v = parse(r#" {"a": [1, -2.5, true, null], "b": {"c": "x\"yé"}, "d": []} "#).unwrap();
        assert_eq!(v.get("a").unwrap().arr().len(), 4);
        assert_eq!(v.get("a").unwrap().arr()[1].num(), Some(-2.5));
        assert_eq!(v.get("b").unwrap().get("c").unwrap().str(), Some("x\"yé"));
        assert!(v.get("d").unwrap().arr().is_empty());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("{\"a\": }").is_none());
        assert!(parse("[1, 2").is_none());
        assert!(parse("{} trailing").is_none());
    }
}
