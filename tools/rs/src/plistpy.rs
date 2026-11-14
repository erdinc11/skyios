use crate::util::die;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

pub type Arr = Rc<RefCell<Vec<V>>>;
pub type Dict = Rc<RefCell<Vec<(String, V)>>>;

#[derive(Clone)]
pub enum V {
    Null,
    Bool(bool),
    Int(i128),
    Real(f64),
    Date(i64),
    Data(Vec<u8>),
    Str(String),
    Uid(Rc<u64>),
    Arr(Arr),
    Dict(Dict),
}

impl V {
    pub fn s(v: &str) -> V {
        V::Str(v.into())
    }
    pub fn strs(v: &[&str]) -> V {
        V::Arr(Rc::new(RefCell::new(v.iter().map(|s| V::s(s)).collect())))
    }
    pub fn dict(items: Vec<(&str, V)>) -> V {
        V::Dict(Rc::new(RefCell::new(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())))
    }
    pub fn as_dict(&self) -> Option<Dict> {
        match self {
            V::Dict(d) => Some(d.clone()),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            V::Str(s) => Some(s),
            _ => None,
        }
    }
}

pub fn get(d: &Dict, k: &str) -> Option<V> {
    d.borrow().iter().find(|(key, _)| key == k).map(|(_, v)| v.clone())
}

pub fn set(d: &Dict, k: &str, v: V) {
    let mut d = d.borrow_mut();
    match d.iter_mut().find(|(key, _)| key == k) {
        Some(e) => e.1 = v,
        None => d.push((k.to_string(), v)),
    }
}

pub fn remove(d: &Dict, k: &str) -> Option<V> {
    let mut d = d.borrow_mut();
    let i = d.iter().position(|(key, _)| key == k)?;
    Some(d.remove(i).1)
}

const EPOCH_2001: i64 = 978307200;

fn secs_to_micros(f: f64) -> i64 {
    let whole = f.trunc();
    whole as i64 * 1_000_000 + ((f - whole) * 1e6).round_ties_even() as i64
}

struct BinReader<'a> {
    b: &'a [u8],
    offs: Vec<usize>,
    ref_size: usize,
    cache: Vec<Option<V>>,
}

fn be_uint(b: &[u8]) -> u64 {
    b.iter().fold(0u64, |a, &c| a << 8 | c as u64)
}

impl<'a> BinReader<'a> {
    fn size(&self, tl: u8, pos: &mut usize) -> usize {
        if tl != 0xf {
            return tl as usize;
        }
        let n = 1usize << (self.b[*pos] & 3);
        let v = be_uint(&self.b[*pos + 1..*pos + 1 + n]) as usize;
        *pos += 1 + n;
        v
    }
    fn refs(&self, n: usize, pos: &mut usize) -> Vec<usize> {
        let r = (0..n).map(|i| be_uint(&self.b[*pos + i * self.ref_size..*pos + (i + 1) * self.ref_size]) as usize).collect();
        *pos += n * self.ref_size;
        r
    }
    fn obj(&mut self, r: usize) -> V {
        if let Some(v) = &self.cache[r] {
            return v.clone();
        }
        let b = self.b;
        let mut pos = self.offs[r];
        let tok = b[pos];
        pos += 1;
        let (th, tl) = (tok & 0xf0, tok & 0x0f);
        let v = match tok {
            0x00 => V::Null,
            0x08 => V::Bool(false),
            0x09 => V::Bool(true),
            0x0f => V::Data(vec![]),
            0x22 => V::Real(f32::from_be_bytes(b[pos..pos + 4].try_into().unwrap()) as f64),
            0x23 => V::Real(f64::from_be_bytes(b[pos..pos + 8].try_into().unwrap())),
            0x33 => V::Date(secs_to_micros(f64::from_be_bytes(b[pos..pos + 8].try_into().unwrap()))),
            _ => match th {
                0x10 => {
                    let n = 1usize << tl;
                    let raw = &b[pos..pos + n];
                    let mut v = 0i128;
                    for &c in raw {
                        v = v << 8 | c as i128;
                    }
                    if tl >= 3 && raw[0] & 0x80 != 0 && n < 16 {
                        v -= 1i128 << (8 * n);
                    }
                    V::Int(v)
                }
                0x40 => {
                    let n = self.size(tl, &mut pos);
                    V::Data(b[pos..pos + n].to_vec())
                }
                0x50 => {
                    let n = self.size(tl, &mut pos);
                    let s = &b[pos..pos + n];
                    if !s.is_ascii() {
                        die("invalid ascii string in binary plist");
                    }
                    V::Str(String::from_utf8(s.to_vec()).unwrap())
                }
                0x60 => {
                    let n = self.size(tl, &mut pos);
                    let units: Vec<u16> = b[pos..pos + 2 * n].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                    V::Str(String::from_utf16(&units).unwrap_or_else(|_| die("invalid utf-16 string in binary plist")))
                }
                0x80 => V::Uid(Rc::new(be_uint(&b[pos..pos + 1 + tl as usize]))),
                0xa0 => {
                    let n = self.size(tl, &mut pos);
                    let refs = self.refs(n, &mut pos);
                    let a: Arr = Rc::new(RefCell::new(vec![]));
                    self.cache[r] = Some(V::Arr(a.clone()));
                    for x in refs {
                        let v = self.obj(x);
                        a.borrow_mut().push(v);
                    }
                    V::Arr(a)
                }
                0xd0 => {
                    let n = self.size(tl, &mut pos);
                    let krefs = self.refs(n, &mut pos);
                    let vrefs = self.refs(n, &mut pos);
                    let d: Dict = Rc::new(RefCell::new(vec![]));
                    self.cache[r] = Some(V::Dict(d.clone()));
                    for (k, o) in krefs.into_iter().zip(vrefs) {
                        let key = match self.obj(k) {
                            V::Str(s) => s,
                            _ => die("non-string dictionary key in binary plist"),
                        };
                        let v = self.obj(o);
                        set(&d, &key, v);
                    }
                    V::Dict(d)
                }
                _ => die(format!("invalid binary plist token {tok:#x}")),
            },
        };
        self.cache[r] = Some(v.clone());
        v
    }
}

fn from_binary(b: &[u8]) -> V {
    if b.len() < 40 {
        die("binary plist too short");
    }
    let t = &b[b.len() - 32..];
    let off_size = t[6] as usize;
    let ref_size = t[7] as usize;
    let n = be_uint(&t[8..16]) as usize;
    let top = be_uint(&t[16..24]) as usize;
    let tbl = be_uint(&t[24..32]) as usize;
    let offs = (0..n).map(|i| be_uint(&b[tbl + i * off_size..tbl + (i + 1) * off_size]) as usize).collect();
    let mut r = BinReader { b, offs, ref_size, cache: vec![None; n] };
    r.obj(top)
}

fn from_crate(v: plist::Value) -> V {
    match v {
        plist::Value::Array(a) => V::Arr(Rc::new(RefCell::new(a.into_iter().map(from_crate).collect()))),
        plist::Value::Dictionary(d) => {
            let out: Dict = Rc::new(RefCell::new(vec![]));
            for (k, v) in d {
                set(&out, &k, from_crate(v));
            }
            V::Dict(out)
        }
        plist::Value::Boolean(b) => V::Bool(b),
        plist::Value::Data(d) => V::Data(d),
        plist::Value::Date(d) => {
            let t: SystemTime = d.into();
            let us = match t.duration_since(UNIX_EPOCH) {
                Ok(x) => x.as_micros() as i64,
                Err(e) => -(e.duration().as_micros() as i64),
            };
            V::Date(us - EPOCH_2001 * 1_000_000)
        }
        plist::Value::Real(f) => V::Real(f),
        plist::Value::Integer(i) => V::Int(i.as_signed().map(|v| v as i128).unwrap_or_else(|| i.as_unsigned().unwrap() as i128)),
        plist::Value::String(s) => V::Str(s),
        plist::Value::Uid(u) => V::Uid(Rc::new(u.get())),
        _ => die("unsupported plist value"),
    }
}

pub fn load(path: impl AsRef<Path>) -> V {
    let p = path.as_ref();
    let b = std::fs::read(p).unwrap_or_else(|e| die(format!("{}: {e}", p.display())));
    if b.starts_with(b"bplist00") {
        return from_binary(&b);
    }
    from_crate(plist::Value::from_reader_xml(std::io::Cursor::new(b)).unwrap_or_else(|e| die(format!("{}: {e}", p.display()))))
}

fn sorted(d: &Dict) -> Vec<(String, V)> {
    let mut items = d.borrow().clone();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
}

#[derive(Hash, PartialEq, Eq, Clone)]
enum Key {
    Null,
    S(String),
    I(i128),
    B(bool),
    R(u64),
    D(i64),
    Data(Vec<u8>),
    Ptr(usize),
}

fn key_of(v: &V) -> Key {
    match v {
        V::Null => Key::Null,
        V::Str(s) => Key::S(s.clone()),
        V::Int(i) => Key::I(*i),
        V::Bool(b) => Key::B(*b),
        V::Real(f) => Key::R(if *f == 0.0 { 0 } else { f.to_bits() }),
        V::Date(d) => Key::D(*d),
        V::Data(d) => Key::Data(d.clone()),
        V::Uid(u) => Key::Ptr(Rc::as_ptr(u) as usize),
        V::Arr(a) => Key::Ptr(Rc::as_ptr(a) as *const u8 as usize),
        V::Dict(d) => Key::Ptr(Rc::as_ptr(d) as *const u8 as usize),
    }
}

struct Bin {
    objs: Vec<V>,
    table: HashMap<Key, usize>,
    out: Vec<u8>,
    ref_size: usize,
}

fn count_to_size(n: usize) -> usize {
    match n as u64 {
        n if n < 1 << 8 => 1,
        n if n < 1 << 16 => 2,
        n if n < 1 << 32 => 4,
        _ => 8,
    }
}

impl Bin {
    fn flatten(&mut self, v: &V) {
        let k = key_of(v);
        if self.table.contains_key(&k) {
            return;
        }
        self.table.insert(k, self.objs.len());
        self.objs.push(v.clone());
        match v {
            V::Dict(d) => {
                let items = sorted(d);
                for (k, _) in &items {
                    self.flatten(&V::Str(k.clone()));
                }
                for (_, v) in &items {
                    self.flatten(v);
                }
            }
            V::Arr(a) => {
                for v in a.borrow().iter() {
                    self.flatten(v);
                }
            }
            _ => {}
        }
    }

    fn refnum(&self, v: &V) -> usize {
        self.table[&key_of(v)]
    }

    fn write_ref(&mut self, r: usize) {
        let b = (r as u64).to_be_bytes();
        self.out.extend(&b[8 - self.ref_size..]);
    }

    fn write_size(&mut self, token: u8, size: usize) {
        if size < 15 {
            self.out.push(token | size as u8);
        } else if size < 1 << 8 {
            self.out.extend([token | 0xf, 0x10, size as u8]);
        } else if size < 1 << 16 {
            self.out.extend([token | 0xf, 0x11]);
            self.out.extend((size as u16).to_be_bytes());
        } else if (size as u64) < 1 << 32 {
            self.out.extend([token | 0xf, 0x12]);
            self.out.extend((size as u32).to_be_bytes());
        } else {
            self.out.extend([token | 0xf, 0x13]);
            self.out.extend((size as u64).to_be_bytes());
        }
    }

    fn write_object(&mut self, v: &V) {
        match v {
            V::Null => self.out.push(0x00),
            V::Bool(false) => self.out.push(0x08),
            V::Bool(true) => self.out.push(0x09),
            V::Int(n) => {
                let n = *n;
                if n < 0 {
                    self.out.push(0x13);
                    self.out.extend((n as i64).to_be_bytes());
                } else if n < 1 << 8 {
                    self.out.extend([0x10, n as u8]);
                } else if n < 1 << 16 {
                    self.out.push(0x11);
                    self.out.extend((n as u16).to_be_bytes());
                } else if n < 1 << 32 {
                    self.out.push(0x12);
                    self.out.extend((n as u32).to_be_bytes());
                } else if n < 1 << 63 {
                    self.out.push(0x13);
                    self.out.extend((n as u64).to_be_bytes());
                } else {
                    self.out.push(0x14);
                    self.out.extend(n.to_be_bytes());
                }
            }
            V::Real(f) => {
                self.out.push(0x23);
                self.out.extend(f.to_be_bytes());
            }
            V::Date(us) => {
                self.out.push(0x33);
                self.out.extend((*us as f64 / 1e6).to_be_bytes());
            }
            V::Data(d) => {
                self.write_size(0x40, d.len());
                self.out.extend(d);
            }
            V::Str(s) if s.is_ascii() => {
                self.write_size(0x50, s.len());
                self.out.extend(s.as_bytes());
            }
            V::Str(s) => {
                let units: Vec<u16> = s.encode_utf16().collect();
                self.write_size(0x60, units.len());
                for u in units {
                    self.out.extend(u.to_be_bytes());
                }
            }
            V::Uid(u) => {
                let n = **u;
                if n < 1 << 8 {
                    self.out.extend([0x80, n as u8]);
                } else if n < 1 << 16 {
                    self.out.push(0x81);
                    self.out.extend((n as u16).to_be_bytes());
                } else if n < 1 << 32 {
                    self.out.push(0x82);
                    self.out.extend((n as u32).to_be_bytes());
                } else {
                    self.out.push(0x83);
                    self.out.extend(n.to_be_bytes());
                }
            }
            V::Arr(a) => {
                let refs: Vec<usize> = a.borrow().iter().map(|v| self.refnum(v)).collect();
                self.write_size(0xa0, refs.len());
                for r in refs {
                    self.write_ref(r);
                }
            }
            V::Dict(d) => {
                let items = sorted(d);
                let krefs: Vec<usize> = items.iter().map(|(k, _)| self.refnum(&V::Str(k.clone()))).collect();
                let vrefs: Vec<usize> = items.iter().map(|(_, v)| self.refnum(v)).collect();
                self.write_size(0xd0, krefs.len());
                for r in krefs.into_iter().chain(vrefs) {
                    self.write_ref(r);
                }
            }
        }
    }
}

pub fn to_binary(value: &V) -> Vec<u8> {
    let mut w = Bin { objs: vec![], table: HashMap::new(), out: vec![], ref_size: 1 };
    w.flatten(value);
    let n = w.objs.len();
    w.ref_size = count_to_size(n);
    w.out.extend(b"bplist00");
    let mut offsets = vec![0usize; n];
    let objs = std::mem::take(&mut w.objs);
    for (i, o) in objs.iter().enumerate() {
        offsets[i] = w.out.len();
        w.write_object(o);
    }
    let top = w.refnum(value);
    let table_off = w.out.len();
    let off_size = count_to_size(table_off);
    for o in offsets {
        w.out.extend(&(o as u64).to_be_bytes()[8 - off_size..]);
    }
    w.out.extend([0u8; 6]);
    w.out.extend([off_size as u8, w.ref_size as u8]);
    for v in [n as u64, top as u64, table_off as u64] {
        w.out.extend(v.to_be_bytes());
    }
    w.out
}

pub fn py_float(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-inf" } else { "inf" }.into();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let shortest = format!("{:e}", f.abs());
    let ndigits = shortest.split_once('e').unwrap().0.chars().filter(|c| c.is_ascii_digit()).count();
    let e = format!("{:.*e}", ndigits - 1, f.abs());
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;
    let decpt = exp + 1;
    let sign = if f < 0.0 { "-" } else { "" };
    let body = if decpt > -4 && decpt <= 16 {
        if decpt <= 0 {
            format!("0.{}{digits}", "0".repeat(-decpt as usize))
        } else if decpt >= n {
            format!("{digits}{}.0", "0".repeat((decpt - n) as usize))
        } else {
            format!("{}.{}", &digits[..decpt as usize], &digits[decpt as usize..])
        }
    } else {
        let m = if n > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits.clone() };
        let x = decpt - 1;
        format!("{m}e{}{:02}", if x < 0 { '-' } else { '+' }, x.abs())
    };
    format!("{sign}{body}")
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn escape(text: &str) -> String {
    if text.chars().any(|c| (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r')) {
        die("strings can't contain control characters; use bytes instead");
    }
    text.replace("\r\n", "\n").replace('\r', "\n").replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

fn date_string(us: i64) -> String {
    let secs = us.div_euclid(1_000_000) + EPOCH_2001;
    let (y, m, d) = civil(secs.div_euclid(86400));
    let t = secs.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

struct Xml {
    out: Vec<u8>,
    level: usize,
}

impl Xml {
    fn line(&mut self, s: &str) {
        if !s.is_empty() {
            self.out.extend("\t".repeat(self.level).as_bytes());
            self.out.extend(s.as_bytes());
        }
        self.out.push(b'\n');
    }
    fn simple(&mut self, el: &str, v: Option<&str>) {
        match v {
            Some(v) => self.line(&format!("<{el}>{}</{el}>", escape(v))),
            None => self.line(&format!("<{el}/>")),
        }
    }
    fn begin(&mut self, el: &str) {
        self.line(&format!("<{el}>"));
        self.level += 1;
    }
    fn end(&mut self, el: &str) {
        self.level -= 1;
        self.line(&format!("</{el}>"));
    }
    fn value(&mut self, v: &V) {
        match v {
            V::Str(s) => self.simple("string", Some(s)),
            V::Bool(true) => self.simple("true", None),
            V::Bool(false) => self.simple("false", None),
            V::Int(i) => self.simple("integer", Some(&i.to_string())),
            V::Real(f) => self.simple("real", Some(&py_float(*f))),
            V::Dict(d) if d.borrow().is_empty() => self.simple("dict", None),
            V::Dict(d) => {
                self.begin("dict");
                for (k, v) in sorted(d) {
                    self.simple("key", Some(&k));
                    self.value(&v);
                }
                self.end("dict");
            }
            V::Data(data) => {
                self.begin("data");
                self.level -= 1;
                let maxline = 16.max(76 - 8 * self.level as i64) as usize;
                for c in data.chunks(maxline / 4 * 3) {
                    self.line(&b64(c));
                }
                self.level += 1;
                self.end("data");
            }
            V::Date(us) => self.simple("date", Some(&date_string(*us))),
            V::Arr(a) if a.borrow().is_empty() => self.simple("array", None),
            V::Arr(a) => {
                self.begin("array");
                for v in a.borrow().iter() {
                    self.value(v);
                }
                self.end("array");
            }
            V::Null | V::Uid(_) => die("unsupported type in XML plist"),
        }
    }
}

pub fn to_xml(value: &V) -> Vec<u8> {
    let mut w = Xml { out: vec![], level: 0 };
    w.out.extend(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n");
    w.line("<plist version=\"1.0\">");
    w.value(value);
    w.line("</plist>");
    w.out
}
