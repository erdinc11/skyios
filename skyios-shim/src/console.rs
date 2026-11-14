use crate::font::FONT;

pub const FB_BASE: u64 = 0x9c00_0000;
pub const FB_W: usize = 1080;
pub const FB_H: usize = 2400;
pub const FB_STRIDE: usize = FB_W * 4;

pub const RED: u32 = 0xffff_0000;
pub const BLUE: u32 = 0xff00_00a0;
const WHITE: u32 = 0xffff_ffff;
const YELLOW: u32 = 0xffff_ff00;

const SCALE: usize = 3;
const CH_W: usize = 8 * SCALE;
const CH_H: usize = 16 * SCALE;
const MARGIN: usize = 24;
const LABEL_COLS: usize = 8;

static mut BG: u32 = 0;
static mut ROW: usize = 0;

fn px(x: usize, y: usize, c: u32) {
    unsafe { core::ptr::write_volatile((FB_BASE as *mut u32).add(y * (FB_STRIDE / 4) + x), c) };
}

pub fn clear(bg: u32) {
    for y in 0..FB_H {
        for x in 0..FB_W {
            px(x, y, bg);
        }
    }
    unsafe {
        BG = bg;
        ROW = 0;
    }
}

pub fn skip() {
    unsafe { ROW += 1 };
}

fn put_char(col: usize, ch: u8, fg: u32) {
    let ch = if (0x20..=0x7e).contains(&ch) { ch } else { b'?' };
    let g = &FONT[(ch - 0x20) as usize];
    let (bg, row) = unsafe { (BG, ROW) };
    let (x0, y0) = (MARGIN + col * CH_W, MARGIN + row * CH_H);
    if x0 + CH_W > FB_W || y0 + CH_H > FB_H {
        return;
    }
    for (gy, bits) in g.iter().enumerate() {
        for gx in 0..8 {
            let c = if bits & (0x80 >> gx) != 0 { fg } else { bg };
            for sy in 0..SCALE {
                for sx in 0..SCALE {
                    px(x0 + gx * SCALE + sx, y0 + gy * SCALE + sy, c);
                }
            }
        }
    }
}

fn put_str(mut col: usize, s: &[u8], fg: u32) -> usize {
    for &b in s {
        if b == 0 {
            break;
        }
        put_char(col, b, fg);
        col += 1;
    }
    col
}

fn hex(v: u64, digits: usize, buf: &mut [u8; 16]) -> &[u8] {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    for i in 0..digits {
        buf[i] = H[((v >> (4 * (digits - 1 - i))) & 0xf) as usize];
    }
    &buf[..digits]
}

pub fn str(label: &[u8], s: &[u8]) {
    put_str(0, label, YELLOW);
    put_str(LABEL_COLS, s, WHITE);
    skip();
}

pub fn hex64(label: &[u8], v: u64) {
    let mut b = [0u8; 16];
    str(label, hex(v, 16, &mut b));
}

pub fn hex2(label: &[u8], a: &[u8], va: u32, b: Option<(&[u8], u32)>) {
    let mut buf = [0u8; 16];
    let mut col = put_str(0, label, YELLOW);
    col = if col < LABEL_COLS { LABEL_COLS } else { col + 1 };
    col = put_str(col, a, YELLOW);
    col = put_str(col, hex(va as u64, 8, &mut buf), WHITE);
    if let Some((bl, vb)) = b {
        col = put_str(col + 1, bl, YELLOW);
        put_str(col, hex(vb as u64, 8, &mut buf), WHITE);
    }
    skip();
}
