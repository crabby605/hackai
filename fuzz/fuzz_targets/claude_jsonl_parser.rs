#![no_main]

use hackai_parser::parse_claude_jsonl;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let _ = parse_claude_jsonl(input, (), None::<()>, "fuzz".into());
    }
});
