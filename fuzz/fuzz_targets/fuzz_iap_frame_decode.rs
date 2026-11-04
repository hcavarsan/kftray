#![no_main]

use kftray_ssh::transport::iap::frame::decode;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((_, consumed)) = decode(data) {
        assert!(consumed <= data.len());
    }
});
