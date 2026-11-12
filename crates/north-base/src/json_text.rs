pub fn ascii_string(text: &str) -> String {
    let encoded = serde_json::to_string(text).expect("a string encodes");
    let mut out = String::with_capacity(encoded.len());
    for character in encoded.chars() {
        if character.is_ascii() {
            out.push(character);
        } else {
            let mut units = [0u16; 2];
            for unit in character.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}
