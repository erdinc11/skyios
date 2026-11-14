use crate::util::{cstr_at, die, py_repr, read, u32le, u64le, write};
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Clone, Default)]
pub struct Node {
    pub props: Vec<(String, u32, Vec<u8>)>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn name(&self) -> String {
        match self.props.iter().find(|p| p.0 == "name") {
            Some(p) => cstr_at(&p.2),
            None => "?".into(),
        }
    }
    pub fn prop(&self, name: &str) -> Option<&[u8]> {
        self.props.iter().find(|p| p.0 == name).map(|p| p.2.as_slice())
    }
    pub fn child_mut(&mut self, name: &str) -> Option<&mut Node> {
        self.children.iter_mut().find(|c| c.name() == name)
    }
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name() == name)
    }
}

fn der(buf: &[u8], off: usize) -> (u8, usize, usize) {
    let tag = buf[off];
    let mut ln = buf[off + 1] as usize;
    let mut off = off + 2;
    if ln & 0x80 != 0 {
        let n = ln & 0x7f;
        ln = buf[off..off + n].iter().fold(0usize, |a, &b| a << 8 | b as usize);
        off += n;
    }
    (tag, off, off + ln)
}

fn im4p_payload(buf: &[u8]) -> (Vec<u8>, bool) {
    let (tag, s, e) = der(buf, 0);
    if tag != 0x30 {
        die("not a DER sequence");
    }
    let mut items = vec![];
    let mut off = s;
    while off < e {
        let (t, vs, ve) = der(buf, off);
        items.push((t, &buf[vs..ve]));
        off = ve;
    }
    if items[0].1 != b"IM4P" {
        die("not an IM4P");
    }
    let encrypted = items.iter().skip(4).any(|(t, _)| *t == 0x04);
    let mut payload = items[3].1.to_vec();
    if payload.starts_with(b"bvx2") {
        let mut child = Command::new("lzfse")
            .arg("-decode")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| die(format!("lzfse: {e}")));
        let mut stdin = child.stdin.take().unwrap();
        let input = payload.clone();
        let w = std::thread::spawn(move || stdin.write_all(&input));
        let out = child.wait_with_output().unwrap_or_else(|e| die(format!("lzfse: {e}")));
        w.join().unwrap().unwrap_or_else(|e| die(format!("lzfse: {e}")));
        if !out.status.success() {
            die("lzfse -decode failed");
        }
        payload = out.stdout;
    }
    (payload, encrypted)
}

pub fn load(path: &str) -> Vec<u8> {
    let buf = read(path);
    if buf.first() == Some(&0x30) && buf[..buf.len().min(16)].windows(4).any(|w| w == b"IM4P") {
        let (p, encrypted) = im4p_payload(&buf);
        if encrypted {
            die("payload is encrypted (KBAG present)");
        }
        return p;
    }
    buf
}

pub fn parse(buf: &[u8], off: usize) -> (Node, usize) {
    let mut node = Node::default();
    let nprops = u32le(buf, off);
    let nchildren = u32le(buf, off + 4);
    let mut off = off + 8;
    for _ in 0..nprops {
        let name = cstr_at(&buf[off..off + 32]);
        let size = u32le(buf, off + 32);
        let ln = (size & 0x7fffffff) as usize;
        node.props.push((name, size >> 31, buf[off + 36..off + 36 + ln].to_vec()));
        off += 36 + ((ln + 3) & !3);
    }
    for _ in 0..nchildren {
        let (child, o) = parse(buf, off);
        node.children.push(child);
        off = o;
    }
    (node, off)
}

pub fn serialize(node: &Node) -> Vec<u8> {
    let mut out = vec![];
    serialize_into(node, &mut out);
    out
}

fn serialize_into(node: &Node, out: &mut Vec<u8>) {
    out.extend((node.props.len() as u32).to_le_bytes());
    out.extend((node.children.len() as u32).to_le_bytes());
    for (name, flags, val) in &node.props {
        let mut n = name.as_bytes().to_vec();
        n.resize(32, 0);
        out.extend(&n);
        out.extend((val.len() as u32 | flags << 31).to_le_bytes());
        out.extend(val);
        out.resize(out.len() + (val.len().wrapping_neg() & 3), 0);
    }
    for c in &node.children {
        serialize_into(c, out);
    }
}

fn hexlist(v: &[u8], w: usize) -> String {
    let items: Vec<String> = v
        .chunks(w)
        .map(|c| format!("{:#x}", if w == 8 { u64le(c, 0) } else { u32le(c, 0) as u64 }))
        .collect();
    format!("<{}>", items.join(" "))
}

fn fmt(v: &[u8], hexall: bool) -> String {
    if v.len() > 1
        && v[v.len() - 1] == 0
        && v[..v.len() - 1].iter().all(|&c| c == 0 || (32..127).contains(&c))
    {
        let end = v.iter().rposition(|&c| c != 0).map_or(0, |i| i + 1);
        let s: String = v[..end].iter().map(|&c| if c == 0 { '|' } else { c as char }).collect();
        return py_repr(&s);
    }
    if (v.len() == 4 || v.len() == 8) && !hexall {
        let mut b = [0u8; 8];
        b[..v.len()].copy_from_slice(v);
        return format!("{:#x}", u64::from_le_bytes(b));
    }
    if v.len().is_multiple_of(8) && v.len() <= 64 {
        return hexlist(v, 8);
    }
    if v.len().is_multiple_of(4) && v.len() <= 64 {
        return hexlist(v, 4);
    }
    let h: String = v[..v.len().min(48)].iter().map(|b| format!("{b:02x}")).collect();
    format!("{h}{} ({} bytes)", if v.len() > 48 { "..." } else { "" }, v.len())
}

pub fn dump(node: &Node, depth: usize, hexall: bool) {
    let ind = "  ".repeat(depth);
    println!("{ind}{} {{", node.name());
    for (n, flags, v) in &node.props {
        if n == "name" {
            continue;
        }
        println!("{ind}  {n}{} = {}", if *flags != 0 { " [placeholder]" } else { "" }, fmt(v, hexall));
    }
    for c in &node.children {
        dump(c, depth + 1, hexall);
    }
    println!("{ind}}}");
}

pub fn main(args: &[String]) -> i32 {
    match args.first().map(|s| s.as_str()) {
        Some("dump") if args.len() >= 2 => {
            let (root, _) = parse(&load(&args[1]), 0);
            dump(&root, 0, args.iter().any(|a| a == "--hex"));
        }
        Some("unwrap") if args.len() >= 3 => write(&args[2], &load(&args[1])),
        _ => {
            eprintln!("usage: sweet adt dump FILE [--hex] | sweet adt unwrap IM4P OUT");
            return 2;
        }
    }
    0
}
