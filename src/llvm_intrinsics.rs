use std::ffi::CStr;

use cstr_ops::CStrExt;

/// Retrieves the handler for an llvm intrinsic function (if any)
pub fn get_instrinsic_handler(name: &CStr) -> Option<fn () -> ()> {
    if name.starts_with(b"llvm.") {
        let nops = [
            &b"llvm.lifetime.start"[..],
            b"llvm.lifetime.end",
        ];
        if nops.iter().any(|nop| name.starts_with(*nop)) {
            return Some(||());
        }
        return Some(|| todo!());
    }
    return None;
}