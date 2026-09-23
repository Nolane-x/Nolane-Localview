#[test]
fn managed_type_text_executor_never_echoes_plaintext_value() {
    let source = include_str!("../src/lib.rs");
    let start = source
        .find("case 'type_text':")
        .expect("managed WebView TypeText executor case");
    let end = source[start..]
        .find("case 'key':")
        .map(|offset| start + offset)
        .expect("TypeText case must end before key case");
    let block = &source[start..end];

    assert!(
        block.contains("setElementValue(target"),
        "TypeText must still execute through the bounded element-value helper"
    );
    assert!(
        block.contains("return { reference: queued.reference };"),
        "TypeText executor may acknowledge only the stable reference"
    );
    assert!(
        !block.contains("value:"),
        "TypeText executor must never echo typed plaintext in completion payload"
    );
}
