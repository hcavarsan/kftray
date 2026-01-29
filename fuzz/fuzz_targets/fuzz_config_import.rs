#![no_main]

use kftray_commons::models::config_model::Config;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(json_str) = std::str::from_utf8(data) {
        let _ = serde_json::from_str::<Config>(json_str);
        let _ = serde_json::from_str::<Vec<Config>>(json_str);
    }
});
