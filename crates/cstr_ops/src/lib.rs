#![feature(cstr_bytes)]

use std::ffi::CStr;

// Yeah they're intentional. This seems to work and I'm too lazy for a marker trait
#[allow(private_bounds)]
pub trait CStrExt {
    /// Tests if a string starts with a given substring
    fn starts_with<P: CPattern>(&self, substring: P) -> bool;

    /// Tests if two strings are equal
    fn equals<P: CPattern>(&self, other: P) -> bool;
}

#[allow(private_bounds)]
impl CStrExt for &CStr {
    fn starts_with<P: CPattern>(&self, pat: P) -> bool {
        pat.is_prefix_of(self)
    }
    
    fn equals<P: CPattern>(&self, other: P) -> bool {
        other.equals(self)
    }
}

trait CPattern {
    fn is_prefix_of(self, cstr: &CStr) -> bool;
    fn equals(self, cstr: &CStr) -> bool;
}

impl CPattern for &[u8] {
    fn is_prefix_of(self, cstr: &CStr) -> bool {
        let mut i = 0;
        for b in cstr.bytes() {
            if i >= self.len() {
                return true;
            }
            if self[i] != b {
                return false;
            }
            i += 1;
        }
        return i == self.len();
    }
    
    fn equals(self, cstr: &CStr) -> bool {
        cstr.bytes().eq(self.iter().cloned())
    }
}

impl <const N: usize> CPattern for &[u8; N] {
    fn is_prefix_of(self, cstr: &CStr) -> bool {
        (&self[..]).is_prefix_of(cstr)
    }
    
    fn equals(self, cstr: &CStr) -> bool {
        CPattern::equals(&self[..], cstr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cstr_startswith() {
        let cstr = c"abc_def";
        assert!(cstr.starts_with(b"abc"));
        assert!(cstr.starts_with(b"abc_def"));
        assert!(cstr.starts_with(b""));
        assert!(!cstr.starts_with(b"beep"));
        assert!(!cstr.starts_with(b"def"));
        assert!(!cstr.starts_with(b"abc_defe"));
    }
    
    #[test]
    fn cstr_eq() {
        let cstr = c"abc_def";
        assert!(cstr.equals(b"abc_def"));
        assert!(!cstr.equals(b"abc_defe"));
    }
}
