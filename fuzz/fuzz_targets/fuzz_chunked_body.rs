#![no_main]

use kftray_http_logs::parser::RequestParser;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = RequestParser::process_chunked_body(data);
});
