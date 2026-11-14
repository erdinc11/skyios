use crate::arch::{dsb, rd32, ticks_virtual, wr32};
use crate::console;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_bytes};

const UFS_BASE: usize = 0x01d8_4000;
const REG_CAP: usize = 0x00;
const REG_VER: usize = 0x08;
const REG_IS: usize = 0x20;
const REG_HCS: usize = 0x30;
const REG_HCE: usize = 0x34;
const REG_UTRLBA: usize = 0x50;
const REG_UTRLBAU: usize = 0x54;
const REG_UTRLDBR: usize = 0x58;
const REG_UTRLCLR: usize = 0x5c;
const REG_UTRLRSR: usize = 0x60;
const HCS_DP: u32 = 1 << 0;
const HCS_UTRLRDY: u32 = 1 << 1;
const BLOCK: u32 = 4096;

#[repr(C)]
#[derive(Clone, Copy)]
struct Utrd {
    dw0: u32,
    dw1: u32,
    dw2: u32,
    dw3: u32,
    ucdba: u32,
    ucdbau: u32,
    resp: u32,
    prdt: u32,
}

#[repr(C, align(1024))]
struct Utrl([Utrd; 32]);

#[repr(C, align(128))]
struct Ucd {
    cmd: [u8; 512],
    resp: [u8; 512],
    prdt: [[u32; 4]; 1],
}

#[repr(C, align(4096))]
struct Data([u8; 2 * BLOCK as usize]);

#[repr(C, align(4096))]
struct Entries([u8; 128 * 128]);

static mut UTRL: Utrl = Utrl([Utrd { dw0: 0, dw1: 0, dw2: 0, dw3: 0, ucdba: 0, ucdbau: 0, resp: 0, prdt: 0 }; 32]);
static mut UCD: Ucd = Ucd { cmd: [0; 512], resp: [0; 512], prdt: [[0; 4]; 1] };
static mut DATA: Data = Data([0; 2 * BLOCK as usize]);
static mut ENTS: Entries = Entries([0; 128 * 128]);
static mut TAG: u8 = 0;

fn rd(off: usize) -> u32 {
    rd32(UFS_BASE + off)
}

fn wr(off: usize, v: u32) {
    wr32(UFS_BASE + off, v)
}

fn be32(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}

fn le64(p: &[u8]) -> u64 {
    u64::from_le_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]])
}

fn data() -> &'static [u8] {
    unsafe { &*addr_of!(DATA.0) }
}

fn scsi(lun: u8, cdb: &[u8], buf: *mut u8, len: u32, ocs: &mut u32, status: &mut u32) -> u32 {
    unsafe {
        let utrd = addr_of_mut!(UTRL.0[0]);
        let ucd = addr_of_mut!(UCD);
        write_bytes(utrd as *mut u8, 0, size_of::<Utrd>());
        write_bytes(ucd as *mut u8, 0, size_of::<Ucd>());
        TAG = TAG.wrapping_add(1);
        let cmd = &mut (*ucd).cmd;
        cmd[0] = 0x01;
        cmd[1] = if len != 0 { 0x40 } else { 0 };
        cmd[2] = lun;
        cmd[3] = TAG;
        cmd[12..16].copy_from_slice(&len.to_be_bytes());
        cmd[16..16 + cdb.len()].copy_from_slice(cdb);
        let ucd_pa = ucd as u64;
        (*utrd).dw0 = (1 << 28) | (if len != 0 { 2 } else { 0 } << 25) | (1 << 24);
        (*utrd).dw2 = 0x0f;
        (*utrd).ucdba = ucd_pa as u32;
        (*utrd).ucdbau = (ucd_pa >> 32) as u32;
        (*utrd).resp = (512 / 4) << 16 | (512 / 4);
        if len != 0 {
            let buf_pa = buf as u64;
            (*ucd).prdt[0] = [buf_pa as u32, (buf_pa >> 32) as u32, 0, len - 1];
            (*utrd).prdt = (1024 / 4) << 16 | 1;
        }
        dsb();
        wr(REG_UTRLDBR, 1);
        let start = ticks_virtual();
        while rd(REG_UTRLDBR) & 1 != 0 {
            if ticks_virtual() - start > 19_200_000 {
                wr(REG_UTRLCLR, !1);
                return 1;
            }
        }
        dsb();
        *ocs = read_volatile(addr_of!((*utrd).dw2)) & 0xff;
        *status = read_volatile(addr_of!((*ucd).resp[7])) as u32;
        if *ocs != 0 {
            return 2;
        }
        if read_volatile(addr_of!((*ucd).resp[0])) != 0x21 {
            return 3;
        }
        if *status != 0 { 4 } else { 0 }
    }
}

fn read_blocks(lba: u32, count: u32, buf: *mut u8) -> u32 {
    let l = lba.to_be_bytes();
    let c = (count as u16).to_be_bytes();
    let cdb = [0x28, 0, l[0], l[1], l[2], l[3], 0, c[0], c[1], 0];
    let (mut ocs, mut st) = (0, 0);
    scsi(0, &cdb, buf, count * BLOCK, &mut ocs, &mut st)
}

fn fs_type(lba: u32) -> &'static [u8] {
    if read_blocks(lba, 1, unsafe { addr_of_mut!(DATA.0) } as *mut u8) != 0 {
        return b"read error";
    }
    let sb = &data()[1024..];
    if sb[..4] == [0x10, 0x20, 0xf5, 0xf2] {
        b"f2fs"
    } else if sb[56] == 0x53 && sb[57] == 0xef {
        b"ext4"
    } else {
        b"other"
    }
}

fn list_partitions(entry_lba: u32, nent: u32) {
    let blocks = (nent * 128).div_ceil(BLOCK);
    if read_blocks(entry_lba, blocks, unsafe { addr_of_mut!(ENTS.0) } as *mut u8) != 0 {
        console::str(b"UFS PART", b"read error");
        return;
    }
    let ents = unsafe { &*addr_of!(ENTS.0) };
    for i in 0..nent as usize {
        let e = &ents[i * 128..(i + 1) * 128];
        let (first, last) = (le64(&e[32..]), le64(&e[40..]));
        if first == 0 {
            continue;
        }
        let mut name = [0u8; 16];
        for (c, n) in name.iter_mut().enumerate() {
            let ch = e[56 + 2 * c];
            *n = if (32..127).contains(&ch) { ch } else if ch != 0 { b'?' } else { 0 };
        }
        let mib = (last - first + 1) >> 8;
        if mib < 64 {
            continue;
        }
        console::hex2(&name, b"lba=", first as u32, Some((b"MiB=", mib as u32)));
        let len = name.iter().position(|&b| b == 0).unwrap_or(16);
        if matches!(&name[..len], b"userdata" | b"cust" | b"cache") {
            console::str(b" fs", fs_type(first as u32));
        }
    }
}

pub fn probe() {
    console::str(b"UFS", b"milestone 5 probe (read-only)");
    let (cap, hcs, hce) = (rd(REG_CAP), rd(REG_HCS), rd(REG_HCE));
    console::hex2(b"UFS", b"ver=", rd(REG_VER), Some((b"cap=", cap)));
    console::hex2(b"UFS", b"hce=", hce, Some((b"hcs=", hcs)));
    console::hex2(b"UFS", b"is=", rd(REG_IS), Some((b"rsr=", rd(REG_UTRLRSR))));
    console::hex2(b"UFS", b"utrlba=", rd(REG_UTRLBA), Some((b"hi=", rd(REG_UTRLBAU))));
    console::hex2(b"UFS", b"dbr=", rd(REG_UTRLDBR), Some((b"nutrs=", (cap & 0x1f) + 1)));
    if hce & 1 == 0 || hcs & HCS_DP == 0 || hcs & HCS_UTRLRDY == 0 {
        console::str(b"UFS", b"host not enabled/ready: stop");
        return;
    }
    if rd(REG_UTRLDBR) != 0 {
        console::str(b"UFS", b"requests outstanding: stop");
        return;
    }
    wr(REG_UTRLRSR, 0);
    let list = addr_of!(UTRL) as u64;
    wr(REG_UTRLBA, list as u32);
    wr(REG_UTRLBAU, (list >> 32) as u32);
    wr(REG_UTRLRSR, 1);
    console::hex2(b"UFS", b"ours=", list as u32, Some((b"rsr=", rd(REG_UTRLRSR))));
    if rd(REG_UTRLBA) != list as u32 || rd(REG_UTRLBAU) != (list >> 32) as u32 || rd(REG_UTRLRSR) & 1 == 0 {
        console::str(b"UFS", b"list base not taken: stop");
        return;
    }

    let (mut ocs, mut st) = (0u32, 0u32);
    let buf = unsafe { addr_of_mut!(DATA.0) } as *mut u8;
    for _ in 0..3 {
        let rc = scsi(0, &[0; 6], core::ptr::null_mut(), 0, &mut ocs, &mut st);
        console::hex2(b"UFS TUR", b"rc=", rc, Some((b"ocs/st=", ocs << 8 | st)));
        if rc == 0 {
            break;
        }
    }
    let mut cap10 = [0u8; 10];
    cap10[0] = 0x25;
    let rc = scsi(0, &cap10, buf, 8, &mut ocs, &mut st);
    console::hex2(b"UFS CAP", b"rc=", rc, Some((b"ocs/st=", ocs << 8 | st)));
    if rc == 0 {
        console::hex2(b"UFS LUN0", b"lastlba=", be32(data()), Some((b"blk=", be32(&data()[4..]))));
    }
    let rd10 = [0x28, 0, 0, 0, 0, 0, 0, 0, 2, 0];
    let rc = scsi(0, &rd10, buf, 2 * BLOCK, &mut ocs, &mut st);
    console::hex2(b"UFS READ", b"rc=", rc, Some((b"ocs/st=", ocs << 8 | st)));
    if rc != 0 {
        return;
    }
    let d = data();
    console::hex2(b"UFS MBR", b"sig=", d[510] as u32 | (d[511] as u32) << 8, Some((b"type=", d[450] as u32)));
    let gpt = &d[BLOCK as usize..];
    let mut sig = [0u8; 8];
    for (i, s) in sig.iter_mut().enumerate() {
        *s = if (32..127).contains(&gpt[i]) { gpt[i] } else { b'.' };
    }
    console::str(b"UFS GPT", &sig);
    let nent = gpt[80] as u32 | (gpt[81] as u32) << 8 | (gpt[82] as u32) << 16;
    let esz = gpt[84] as u32 | (gpt[85] as u32) << 8;
    let elba = gpt[72] as u32 | (gpt[73] as u32) << 8;
    console::hex2(b"UFS GPT", b"entries=", nent, Some((b"lba=", elba)));
    if esz != 128 || nent > 128 {
        return;
    }
    list_partitions(elba, nent);
}
