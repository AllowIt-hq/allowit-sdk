use crate::error::quoted;
use serde_json::Value;
pub fn short(s: &str) -> String {
    String::from_utf8_lossy(&s.as_bytes()[..s.len().min(12)]).into_owned()
}
pub fn one_line(s: &str) -> String {
    if s.contains(['\r', '\n', '\u{85}', '\u{2028}', '\u{2029}']) {
        quoted(s)
            .replace('\u{85}', "\\u0085")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    } else {
        s.into()
    }
}
pub fn clean(mut s: String, token: &str) -> String {
    for secret in [token, token.trim()] {
        if !secret.is_empty() {
            s = s.replace(secret, "[redacted]");
        }
    }
    let p: Vec<_> = token.trim().split('.').collect();
    if p.len() == 3 && p[2].len() >= 8 {
        s = s.replace(p[2], "[redacted]");
    }
    s.chars()
        .map(|c| {
            if matches!(c, '\n' | '\t') {
                c
            } else if c < '\u{20}'
                || ('\u{7f}'..'\u{a0}').contains(&c)
                || ('\u{202a}'..='\u{202e}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)
            {
                '?'
            } else {
                c
            }
        })
        .collect()
}
pub fn json(v: &Value) -> String {
    // MPP enables serde_json/preserve_order. Keep the established CLI output
    // ordering while leaving typed request and identity bytes unchanged.
    let mut v = v.clone();
    v.sort_all_objects();
    format!(
        "{}\n",
        serde_json::to_string_pretty(&v)
            .unwrap()
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('&', "\\u0026")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_output_cannot_expose_capabilities_or_terminal_controls() {
        let token = "owner.policy.abcdefghijklmnop";
        let raw = format!("{token} abcdefghijklmnop\u{1b}[31m\u{202e}fake");
        let shown = clean(raw, token);
        assert!(!shown.contains("abcdefghijklmnop"));
        assert!(!shown.contains(['\u{1b}', '\u{202e}']));
    }

    #[test]
    fn json_escapes_markup_without_changing_identity_fields() {
        let value = serde_json::json!({"requestId":"original-0001","body":"<script>&\u{2028}"});
        let shown = json(&value);
        assert!(!shown.contains('<'));
        assert_eq!(serde_json::from_str::<Value>(&shown).unwrap(), value);
    }
}
