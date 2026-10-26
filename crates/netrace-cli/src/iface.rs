//! Interface names as typed: `g0/0`, `gig 0/1`, `fa0/1`, `lo0`, `vlan 1`,
//! `g0/0.10`. Expanded to the full IOS name and checked against the device.

use netrace_doc::DeviceKind;

/// Interface types IOS accepts, with their full names.
const TYPES: [(&str, &str); 5] = [
    ("gigabitethernet", "GigabitEthernet"),
    ("fastethernet", "FastEthernet"),
    ("ethernet", "Ethernet"),
    ("loopback", "Loopback"),
    ("vlan", "Vlan"),
];

/// Types that exist on this device kind, for `?` and Tab.
pub(crate) fn types_for(kind: DeviceKind) -> Vec<(&'static str, &'static str)> {
    let ports = kind.port_names();
    let mut types: Vec<(&str, &str)> = Vec::new();
    for (_, full) in TYPES {
        let present = ports.iter().any(|p| p.starts_with(full))
            || (full == "Loopback" && (kind.is_router() || kind == DeviceKind::L3Switch))
            || (full == "Vlan" && kind.is_switch());
        if present {
            let help = match full {
                "GigabitEthernet" => "GigabitEthernet IEEE 802.3z",
                "FastEthernet" => "FastEthernet IEEE 802.3",
                "Ethernet" => "IEEE 802.3",
                "Loopback" => "Loopback interface",
                _ => "Catalyst Vlans",
            };
            types.push((full, help));
        }
    }
    types
}

/// Splits "gig0/1" into ("gig", "0/1").
fn split(text: &str) -> (&str, &str) {
    let at = text
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(text.len());
    (&text[..at], &text[at..])
}

/// The full type name for a typed prefix ("g" → "GigabitEthernet"), among
/// the types this device has.
pub(crate) fn expand_type(prefix: &str, kind: DeviceKind) -> Option<&'static str> {
    if prefix.is_empty() {
        return None;
    }
    let lower = prefix.to_ascii_lowercase();
    let matches: Vec<&'static str> = types_for(kind)
        .into_iter()
        .filter(|(full, _)| full.to_ascii_lowercase().starts_with(&lower))
        .map(|(full, _)| full)
        .collect();
    match matches.as_slice() {
        [one] => Some(one),
        _ => None,
    }
}

/// Parses an interface typed as one token ("g0/0") and checks the device
/// could have it. Returns the full name.
pub(crate) fn parse(text: &str, kind: DeviceKind) -> Option<String> {
    let (prefix, number) = split(text);
    if number.is_empty()
        || !number
            .chars()
            .all(|c| c.is_ascii_digit() || c == '/' || c == '.')
    {
        return None;
    }
    let full = format!("{}{number}", expand_type(prefix, kind)?);
    kind.accepts_interface(&full).then_some(full)
}

/// Parses "gig" + "0/0" given as two tokens.
pub(crate) fn parse_two(type_text: &str, number: &str, kind: DeviceKind) -> Option<String> {
    if type_text.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    parse(&format!("{type_text}{number}"), kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviations_expand() {
        let r = DeviceKind::Router;
        assert_eq!(parse("g0/0", r).as_deref(), Some("GigabitEthernet0/0"));
        assert_eq!(parse("Gi0/3", r).as_deref(), Some("GigabitEthernet0/3"));
        assert_eq!(parse("lo0", r).as_deref(), Some("Loopback0"));
        assert_eq!(
            parse("g0/1.10", r).as_deref(),
            Some("GigabitEthernet0/1.10")
        );
        assert_eq!(
            parse_two("gig", "0/1", r).as_deref(),
            Some("GigabitEthernet0/1")
        );
        assert_eq!(parse("g0/4", r), None);
        assert_eq!(parse("f0/1", r), None, "routers here have no FastEthernet");
        let sw = DeviceKind::L2Switch;
        assert_eq!(parse("f0/24", sw).as_deref(), Some("FastEthernet0/24"));
        assert_eq!(parse_two("vlan", "1", sw).as_deref(), Some("Vlan1"));
        assert_eq!(parse("int", sw), None);
    }
}
