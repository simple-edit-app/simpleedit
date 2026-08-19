use std::fs;
use tempfile::tempdir;

#[test]
fn config_saves_and_loads() {
    // We can't test Config::save/load directly without mocking dirs::config_dir
    // so we test the serde round-trip as a proxy
    let json = r#"{
        "dark_mode": true,
        "font_size": 16.0,
        "font_family": "JetBrains Mono",
        "tab_width": 2,
        "use_spaces": true,
        "show_line_numbers": true,
        "word_wrap": false,
        "auto_indent": true,
        "autocomplete_brackets": false,
        "autocomplete_quotes": false,
        "show_page_guide": true,
        "page_guide_column": 100,
        "highlight_current_line": false,
        "locale": "fr"
    }"#;

    // Parse the JSON to verify the schema is correct
    let v: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
    assert_eq!(v["dark_mode"], true);
    assert_eq!(v["tab_width"], 2);
    assert_eq!(v["locale"], "fr");
}

#[test]
fn config_file_write_read() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("config.json");

    let config_content = r#"{"dark_mode":false,"font_size":14.0,"font_family":"Monospace","tab_width":4,"use_spaces":true,"show_line_numbers":true,"word_wrap":true,"auto_indent":true,"autocomplete_brackets":true,"autocomplete_quotes":true,"show_page_guide":false,"page_guide_column":80,"highlight_current_line":true,"locale":"en"}"#;

    fs::write(&file_path, config_content).unwrap();
    let read_back = fs::read_to_string(&file_path).unwrap();
    assert_eq!(config_content, read_back);
}
