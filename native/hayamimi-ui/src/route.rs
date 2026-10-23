//! Which page an address is, and which relay stream its query names.

use crate::overlay::Show;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overlay(Show),
    Dashboard,
    Transcript,
}

/// The page for a path and a query string. `/` and every path that is not one
/// of the other two is the overlay, the page a browser source loads.
pub fn page_for(path: &str, search: &str) -> Page {
    match path.trim_end_matches('/') {
        "/dashboard" => Page::Dashboard,
        "/transcript" => Page::Transcript,
        _ => Page::Overlay(Show::from_query(search)),
    }
}

/// The address of a relay's `/events` that a page's query names: the value of
/// the first `relay` key, read as `URLSearchParams.get` reads it, when it
/// starts with `http://` or `https://`. The scheme is compared as written, and
/// the value is not trimmed. Any other value, and no `relay` key, is no relay.
pub fn relay_url(search: &str) -> Option<String> {
    let query = match search.strip_prefix('?') {
        Some(rest) => rest,
        None => search,
    };
    let (_, value) = query
        .split('&')
        .map(split_pair)
        .find(|(key, _)| decode_form(key) == "relay")?;
    let value = decode_form(value);
    (value.starts_with("http://") || value.starts_with("https://")).then_some(value)
}

/// The name and the value of one `name=value` pair of a query.
fn split_pair(pair: &str) -> (&str, &str) {
    match pair.split_once('=') {
        Some(found) => found,
        // DEFAULT: the form-urlencoded parser gives a pair with no `=` an empty value.
        None => (pair, ""),
    }
}

/// Reads `+` as a space and `%XX` as a byte. A `%` that does not begin two hex
/// digits stays as written, and bytes that are not UTF-8 become U+FFFD.
fn decode_form(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            b'%' => match percent_byte(&bytes[at + 1..]) {
                Some(decoded) => {
                    out.push(decoded);
                    at += 3;
                }
                None => {
                    out.push(b'%');
                    at += 1;
                }
            },
            other => {
                out.push(other);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The byte two hex digits at the start of `rest` stand for.
fn percent_byte(rest: &[u8]) -> Option<u8> {
    let [high, low, ..] = rest else {
        return None;
    };
    Some(hex_value(*high)? << 4 | hex_value(*low)?)
}

fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_paths_are_the_three_pages() {
        assert_eq!(page_for("/dashboard", ""), Page::Dashboard);
        assert_eq!(page_for("/transcript", ""), Page::Transcript);
        assert_eq!(page_for("/", ""), Page::Overlay(Show::Both));
        assert_eq!(page_for("/", "?show=final"), Page::Overlay(Show::Final));
        assert_eq!(page_for("/", "?show=partial"), Page::Overlay(Show::Partial));
    }

    #[test]
    fn a_trailing_slash_does_not_change_the_page() {
        assert_eq!(page_for("/dashboard/", ""), Page::Dashboard);
        assert_eq!(page_for("/transcript/", "?show=final"), Page::Transcript);
    }

    #[test]
    fn any_other_path_is_the_overlay() {
        assert_eq!(page_for("", ""), Page::Overlay(Show::Both));
        assert_eq!(page_for("/index.html", ""), Page::Overlay(Show::Both));
        assert_eq!(page_for("/dashboard2", ""), Page::Overlay(Show::Both));
    }

    const RELAY: &str = "http://127.0.0.1:8835/events";

    /// T-IP-05-007
    #[test]
    fn the_relay_address_is_read_from_the_query_and_the_page_stays_the_dashboard() {
        let search = format!("?relay={RELAY}");
        assert_eq!(relay_url(&search).as_deref(), Some(RELAY));
        assert_eq!(page_for("/dashboard", &search), Page::Dashboard);
    }

    /// T-IP-05-008
    #[test]
    fn a_query_without_a_relay_has_none() {
        assert_eq!(relay_url(""), None);
        assert_eq!(relay_url("?show=final"), None);
    }

    /// T-IP-05-009
    #[test]
    fn an_empty_relay_value_is_no_relay() {
        assert_eq!(relay_url("?relay="), None);
    }

    /// T-IP-05-010
    #[test]
    fn a_relay_beside_another_query_key_changes_neither() {
        let search = format!("?show=final&relay={RELAY}");
        assert_eq!(page_for("/", &search), Page::Overlay(Show::Final));
        assert_eq!(relay_url(&search).as_deref(), Some(RELAY));
    }

    /// T-IP-05-011
    #[test]
    fn an_https_relay_and_a_relay_on_another_host_are_read() {
        for value in [
            "https://192.168.0.5:8835/events",
            "http://example.test:8835/events",
        ] {
            let search = format!("?relay={value}");
            assert_eq!(relay_url(&search).as_deref(), Some(value), "{value}");
        }
    }

    /// T-IP-05-012
    #[test]
    fn a_relay_value_with_another_scheme_an_upper_case_scheme_or_none_is_no_relay() {
        for value in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/plain,x",
            "ftp://127.0.0.1/events",
            "//127.0.0.1:8835/events",
            "127.0.0.1:8835/events",
            "HTTP://127.0.0.1:8835/events",
            "Https://127.0.0.1:8835/events",
        ] {
            let search = format!("?relay={value}");
            assert_eq!(relay_url(&search), None, "{value}");
        }
    }

    /// T-IP-05-013
    #[test]
    fn a_percent_encoded_relay_value_is_decoded_once_by_the_form_urlencoded_rules() {
        let cases = [
            (
                "?relay=http%3A%2F%2F127.0.0.1%3A8835%2Fevents",
                "http://127.0.0.1:8835/events",
            ),
            (
                "?relay=http://127.0.0.1:8835/events%3Fa%3Db",
                "http://127.0.0.1:8835/events?a=b",
            ),
            (
                "?relay=http%3A%2F%2Fa.test%2Fe%3Fq%3Db+c",
                "http://a.test/e?q=b c",
            ),
        ];
        for (search, expected) in cases {
            assert_eq!(relay_url(search).as_deref(), Some(expected), "{search}");
        }
    }

    /// T-IP-05-014
    #[test]
    fn the_scheme_is_judged_on_the_value_decoded_exactly_once_and_the_value_is_not_trimmed() {
        for search in [
            "?relay=javascript%3Aalert(1)",
            "?relay=http%253A%252F%252F127.0.0.1%253A8835%252Fevents",
            "?relay=%20http://127.0.0.1:8835/events",
        ] {
            assert_eq!(relay_url(search), None, "{search}");
        }
        let cases = [
            ("?relay=%68ttp://127.0.0.1:8835/events", RELAY),
            (
                "?relay=http://127.0.0.1:8835/events%20",
                "http://127.0.0.1:8835/events ",
            ),
            (
                "?relay=http://127.0.0.1:8835/events%09",
                "http://127.0.0.1:8835/events\t",
            ),
        ];
        for (search, expected) in cases {
            assert_eq!(relay_url(search).as_deref(), Some(expected), "{search}");
        }
    }

    /// T-IP-05-015
    #[test]
    fn only_the_first_relay_key_is_read_and_its_name_must_match_exactly() {
        assert_eq!(relay_url(&format!("?relay=a&relay={RELAY}")), None);
        assert_eq!(
            relay_url(&format!("?relay={RELAY}&relay=a")).as_deref(),
            Some(RELAY)
        );
        for key in ["xrelay", "relay2", "Relay"] {
            assert_eq!(relay_url(&format!("?{key}={RELAY}")), None, "{key}");
        }
        assert_eq!(relay_url("?relay"), None);
    }

    /// T-IP-05-016
    #[test]
    fn an_invalid_percent_sequence_in_the_relay_value_is_kept_as_it_is() {
        for value in ["http://x.test/%zz", "http://x.test/%"] {
            let search = format!("?relay={value}");
            assert_eq!(relay_url(&search).as_deref(), Some(value), "{value}");
        }
    }
}
