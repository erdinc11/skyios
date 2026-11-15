// Qualcomm secure channel manager calls (SMCCC, SIP owner), following Linux
// drivers/firmware/qcom/qcom_scm-smc.c: standard 64-bit calls, arginfo in x1,
// up to four arguments in x2..x5, the QCOM A6 quirk (x6 is a session cookie
// that must be passed back when TZ reports INTERRUPTED) and EBUSY retries.

use crate::xnu::sleep_ms;

const SCM_INTERRUPTED: u64 = 1;
const SCM_V2_EBUSY: i64 = -12;

pub const VAL: u32 = 0;
pub const RW: u32 = 2;

pub fn args(types: &[u32]) -> u64 {
    let mut a = types.len() as u64;
    for (i, &t) in types.iter().enumerate() {
        a |= (t as u64) << (4 + 2 * i);
    }
    a
}

fn fnid(svc: u32, cmd: u32) -> u64 {
    // ARM_SMCCC_CALL_VAL(STD_CALL, SMC_64, OWNER_SIP, (svc << 8) | cmd)
    (1 << 30) | (2 << 24) | (((svc & 0xff) << 8) | (cmd & 0xff)) as u64
}

fn smc(x: &mut [u64; 7]) {
    unsafe {
        core::arch::asm!(
            "smc #0",
            inout("x0") x[0],
            inout("x1") x[1],
            inout("x2") x[2],
            inout("x3") x[3],
            inout("x4") x[4],
            inout("x5") x[5],
            inout("x6") x[6],
            out("x7") _,
            out("x8") _, out("x9") _, out("x10") _, out("x11") _,
            out("x12") _, out("x13") _, out("x14") _, out("x15") _,
            out("x16") _, out("x17") _,
            options(nostack)
        );
    }
}

/// PSCI SYSTEM_RESET (SMCCC fast call); does not return when it works
pub fn psci_system_reset() {
    let mut x = [0x8400_0009, 0, 0, 0, 0, 0, 0];
    smc(&mut x);
}

// Returns (status, result[0..3]); status 0 is success, negative values are
// the SCM error codes (-1 ERROR, -2 EINVAL_ADDR, -3 EINVAL_ARG, -4 ENOTSUPP ...)
pub fn call(svc: u32, cmd: u32, arginfo: u64, a: &[u64]) -> (i64, [u64; 3]) {
    let mut v = [0u64; 4];
    v[..a.len()].copy_from_slice(a);
    for _ in 0..20 {
        let mut x = [fnid(svc, cmd), arginfo, v[0], v[1], v[2], v[3], 0];
        loop {
            smc(&mut x);
            if x[0] != SCM_INTERRUPTED {
                break;
            }
            // resume: x0 = INTERRUPTED, x6 = cookie returned by TZ
            x = [SCM_INTERRUPTED, arginfo, v[0], v[1], v[2], v[3], x[6]];
        }
        if x[0] as i64 != SCM_V2_EBUSY {
            return (x[0] as i64, [x[1], x[2], x[3]]);
        }
        sleep_ms(30);
    }
    (SCM_V2_EBUSY, [0; 3])
}
