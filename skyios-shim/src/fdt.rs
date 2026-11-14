const FDT_MAGIC: u32 = 0xd00d_feed;
const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const NOP: u32 = 4;

fn be32(a: usize) -> u32 {
    u32::from_be(unsafe { core::ptr::read_volatile(a as *const u32) })
}

fn byte(a: usize) -> u8 {
    unsafe { core::ptr::read_volatile(a as *const u8) }
}

fn cstr_eq(a: usize, b: &[u8]) -> bool {
    for (i, &c) in b.iter().enumerate() {
        if byte(a + i) != c {
            return false;
        }
    }
    byte(a + b.len()) == 0
}

fn skip_str(mut p: usize) -> usize {
    while byte(p) != 0 {
        p += 1;
    }
    p + 1
}

fn align4(a: usize) -> usize {
    (a + 3) & !3
}

pub fn chosen_u64(fdt: usize, prop: &[u8]) -> Option<u64> {
    if fdt == 0 || be32(fdt) != FDT_MAGIC {
        return None;
    }
    let mut p = fdt + be32(fdt + 8) as usize;
    let strings = fdt + be32(fdt + 12) as usize;
    let (mut depth, mut in_chosen) = (0, false);
    loop {
        let tok = be32(p);
        p += 4;
        match tok {
            BEGIN_NODE => {
                depth += 1;
                in_chosen = depth == 2 && cstr_eq(p, b"chosen");
                p = align4(skip_str(p));
            }
            END_NODE => {
                if in_chosen {
                    return None;
                }
                depth -= 1;
            }
            PROP => {
                let (len, nameoff) = (be32(p), be32(p + 4));
                let val = p + 8;
                if in_chosen && cstr_eq(strings + nameoff as usize, prop) {
                    return match len {
                        4 => Some(be32(val) as u64),
                        8 => Some((be32(val) as u64) << 32 | be32(val + 4) as u64),
                        _ => None,
                    };
                }
                p = align4(val + len as usize);
            }
            NOP => {}
            _ => return None,
        }
    }
}
