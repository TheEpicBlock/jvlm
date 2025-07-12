#![no_std]

#[unsafe(no_mangle)]
pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    loop {}
}