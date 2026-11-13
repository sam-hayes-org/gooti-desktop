//! Which requests an app may make without asking: its trust level and the choices the user
//! remembered. A remembered deny always wins, as in the extension.

use serde::{Deserialize, Serialize};

/// Kinds that change who you are, what you follow, where you are reached, or sign you in or
/// pay somewhere. They ask unless the app is trusted, and a remembered allow lasts an hour.
pub const SENSITIVE_KINDS: [u16; 13] = [
    0,     // profile
    3,     // follow list
    5,     // deletion
    62,    // erase account (NIP-62)
    10000, // mute list
    10002, // relay list
    10050, // DM relays
    22242, // relay sign-in (NIP-42)
    24242, // Blossom auth
    27235, // HTTP auth (NIP-98)
    13194, // wallet info (NIP-47)
    23194, // wallet request
    23195, // wallet response
];

/// A remembered allow for a sensitive kind lasts at most this long.
pub const SENSITIVE_MAX_MS: i64 = 60 * 60 * 1000;

pub fn is_sensitive(kind: u16) -> bool {
    SENSITIVE_KINDS.contains(&kind)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trust {
    /// Every sign, encrypt and decrypt asks, unless a remembered choice matches.
    #[default]
    Ask,
    /// Routine requests are approved; sensitive kinds ask.
    Basic,
    /// Everything is approved, sensitive kinds included.
    Trust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Policy {
    Allow,
    Deny,
}

/// A choice the user remembered for one app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub method: String,
    /// For `sign_event`: the kind. Encrypt and decrypt rules have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<u16>,
    pub policy: Policy,
    pub created_at: i64,
    /// `None` is always.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

impl Rule {
    fn matches(&self, method: &str, kind: Option<u16>, now: i64) -> bool {
        self.method == method
            && self.expires_at.is_none_or(|expires| now < expires)
            && (self.kind.is_none() || self.kind == kind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Allow,
    Deny,
    Ask,
}

/// Why a request was answered the way it was. Shown in Activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// A remembered choice.
    Remembered,
    /// The app's trust level.
    TrustLevel,
    /// The user, in the approval prompt.
    User,
    /// Nobody answered in time.
    TimedOut,
}

/// What to do with a request: a remembered deny first, then a remembered allow, then the trust level.
pub fn evaluate(
    trust: Trust,
    rules: &[Rule],
    method: &str,
    kind: Option<u16>,
    now: i64,
) -> (Verdict, Reason) {
    let matching = || rules.iter().filter(|rule| rule.matches(method, kind, now));
    if matching().any(|rule| rule.policy == Policy::Deny) {
        return (Verdict::Deny, Reason::Remembered);
    }
    if matching().any(|rule| rule.policy == Policy::Allow) {
        return (Verdict::Allow, Reason::Remembered);
    }
    let sensitive = kind.is_some_and(is_sensitive);
    let verdict = match trust {
        Trust::Trust => Verdict::Allow,
        Trust::Basic if !sensitive => Verdict::Allow,
        Trust::Basic | Trust::Ask => Verdict::Ask,
    };
    (
        verdict,
        if verdict == Verdict::Ask {
            Reason::User
        } else {
            Reason::TrustLevel
        },
    )
}

/// How long to remember a choice, as offered in the approval prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Remember {
    Once,
    FiveMinutes,
    OneHour,
    OneDay,
    Always,
}

/// The rule to keep for a decision, or `None` for "only this time".
/// A sensitive kind is remembered for an hour at most.
pub fn rule_for(
    remember: Remember,
    method: &str,
    kind: Option<u16>,
    policy: Policy,
    now: i64,
) -> Option<Rule> {
    let duration = match remember {
        Remember::Once => return None,
        Remember::FiveMinutes => Some(5 * 60 * 1000),
        Remember::OneHour => Some(60 * 60 * 1000),
        Remember::OneDay => Some(24 * 60 * 60 * 1000),
        Remember::Always => None,
    };
    let duration = if policy == Policy::Allow && kind.is_some_and(is_sensitive) {
        Some(duration.map_or(SENSITIVE_MAX_MS, |d: i64| d.min(SENSITIVE_MAX_MS)))
    } else {
        duration
    };
    Some(Rule {
        method: method.to_string(),
        kind: if method == "sign_event" { kind } else { None },
        policy,
        created_at: now,
        expires_at: duration.map(|d| now + d),
    })
}

/// Adds a rule, replacing an older one for the same method and kind, and drops expired ones.
pub fn remember(rules: &mut Vec<Rule>, rule: Rule, now: i64) {
    rules.retain(|r| {
        r.expires_at.is_none_or(|e| now < e) && !(r.method == rule.method && r.kind == rule.kind)
    });
    rules.push(rule);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(method: &str, kind: Option<u16>, policy: Policy, expires_at: Option<i64>) -> Rule {
        Rule {
            method: method.into(),
            kind,
            policy,
            created_at: 0,
            expires_at,
        }
    }

    #[test]
    fn trust_levels_without_rules() {
        let cases = [
            (Trust::Ask, Some(1), Verdict::Ask),
            (Trust::Ask, None, Verdict::Ask),
            (Trust::Basic, Some(1), Verdict::Allow),
            (Trust::Basic, None, Verdict::Allow),
            (Trust::Basic, Some(0), Verdict::Ask),
            (Trust::Basic, Some(10002), Verdict::Ask),
            (Trust::Trust, Some(0), Verdict::Allow),
        ];
        for (trust, kind, expected) in cases {
            let method = if kind.is_some() {
                "sign_event"
            } else {
                "nip44_decrypt"
            };
            assert_eq!(
                evaluate(trust, &[], method, kind, 0).0,
                expected,
                "{trust:?} {kind:?}"
            );
        }
    }

    #[test]
    fn a_remembered_deny_wins_over_an_allow_and_over_trust() {
        let rules = [
            rule("sign_event", Some(1), Policy::Allow, None),
            rule("sign_event", Some(1), Policy::Deny, None),
        ];
        assert_eq!(
            evaluate(Trust::Trust, &rules, "sign_event", Some(1), 0),
            (Verdict::Deny, Reason::Remembered)
        );
        // A deny for the whole method covers every kind.
        let rules = [
            rule("sign_event", None, Policy::Deny, None),
            rule("sign_event", Some(7), Policy::Allow, None),
        ];
        assert_eq!(
            evaluate(Trust::Ask, &rules, "sign_event", Some(7), 0).0,
            Verdict::Deny
        );
    }

    #[test]
    fn a_remembered_allow_is_per_kind_and_expires() {
        let rules = [rule("sign_event", Some(1), Policy::Allow, Some(100))];
        assert_eq!(
            evaluate(Trust::Ask, &rules, "sign_event", Some(1), 50),
            (Verdict::Allow, Reason::Remembered)
        );
        assert_eq!(
            evaluate(Trust::Ask, &rules, "sign_event", Some(7), 50).0,
            Verdict::Ask
        );
        assert_eq!(
            evaluate(Trust::Ask, &rules, "sign_event", Some(1), 100).0,
            Verdict::Ask
        );
        assert_eq!(
            evaluate(Trust::Ask, &rules, "nip44_decrypt", None, 50).0,
            Verdict::Ask
        );
    }

    #[test]
    fn sensitive_kinds_are_remembered_for_an_hour_at_most() {
        let always =
            rule_for(Remember::Always, "sign_event", Some(0), Policy::Allow, 1000).unwrap();
        assert_eq!(always.expires_at, Some(1000 + SENSITIVE_MAX_MS));
        let day = rule_for(Remember::OneDay, "sign_event", Some(3), Policy::Allow, 0).unwrap();
        assert_eq!(day.expires_at, Some(SENSITIVE_MAX_MS));
        let short = rule_for(
            Remember::FiveMinutes,
            "sign_event",
            Some(0),
            Policy::Allow,
            0,
        )
        .unwrap();
        assert_eq!(short.expires_at, Some(5 * 60 * 1000));
        // A deny may last forever, and routine kinds too.
        assert_eq!(
            rule_for(Remember::Always, "sign_event", Some(0), Policy::Deny, 0)
                .unwrap()
                .expires_at,
            None
        );
        assert_eq!(
            rule_for(Remember::Always, "sign_event", Some(1), Policy::Allow, 0)
                .unwrap()
                .expires_at,
            None
        );
        assert!(rule_for(Remember::Once, "sign_event", Some(1), Policy::Allow, 0).is_none());
    }

    #[test]
    fn encrypt_rules_cover_the_method_and_a_new_rule_replaces_the_old() {
        let rule = rule_for(Remember::Always, "nip44_encrypt", Some(1), Policy::Allow, 0).unwrap();
        assert_eq!(rule.kind, None);
        let mut rules = vec![
            self::rule("sign_event", Some(1), Policy::Allow, None),
            self::rule("sign_event", Some(7), Policy::Allow, Some(10)),
        ];
        remember(
            &mut rules,
            self::rule("sign_event", Some(1), Policy::Deny, None),
            20,
        );
        assert_eq!(
            rules,
            [self::rule("sign_event", Some(1), Policy::Deny, None)]
        );
    }
}
