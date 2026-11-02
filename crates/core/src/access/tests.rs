#![allow(clippy::unwrap_used)]

use super::{Access, Fingerprint, InvalidFingerprint, MemberDirectory, NoMembers, decide_access};

const VALID: &str = "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s";

struct OnlyKey(Fingerprint);

impl MemberDirectory for OnlyKey {
    fn is_member(&self, key: &Fingerprint) -> impl Future<Output = bool> + Send {
        std::future::ready(*key == self.0)
    }
}

fn fingerprint(text: &str) -> Fingerprint {
    Fingerprint::parse(text).unwrap()
}

#[tokio::test]
async fn unknown_key_is_restricted() {
    let access = decide_access(&NoMembers, &fingerprint(VALID)).await;
    assert_eq!(access, Access::Restricted);
}

#[tokio::test]
async fn known_member_key_gets_member_access() {
    let directory = OnlyKey(fingerprint(VALID));
    assert_eq!(
        decide_access(&directory, &fingerprint(VALID)).await,
        Access::Member
    );
}

#[tokio::test]
async fn a_different_key_is_still_restricted() {
    let directory = OnlyKey(fingerprint(VALID));
    let other = fingerprint("SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(decide_access(&directory, &other).await, Access::Restricted);
}

#[test]
fn fingerprint_parse_accepts_openssh_sha256() {
    assert_eq!(fingerprint(VALID).as_str(), VALID);
}

#[test]
fn fingerprint_parse_rejects_bad_shapes() {
    for bad in [
        "",
        "SHA256:",
        "MD5:aa:bb",
        "uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s",
        "SHA256:short",
        "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s=",
        "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2\x1b",
    ] {
        assert_eq!(Fingerprint::parse(bad), Err(InvalidFingerprint), "{bad:?}");
    }
}

#[test]
fn placeholder_permissions_have_unique_area_verb_names() {
    use super::Permission;
    let mut seen = std::collections::HashSet::new();
    for permission in Permission::ALL {
        let name = permission.name();
        assert!(seen.insert(name), "{name} is listed twice");
        let (area, verb) = name.split_once('.').unwrap();
        assert!(!area.is_empty() && !verb.is_empty(), "{name}");
    }
}

#[test]
fn a_member_lacks_approval_and_a_sysop_holds_everything() {
    use super::{Permission, PermissionCheck, RolePermissions};
    use crate::store::Role;
    let member = RolePermissions::new(Role::Member);
    let sysop = RolePermissions::new(Role::Sysop);
    for permission in Permission::ALL {
        assert!(sysop.allows(permission), "{}", permission.name());
        assert_eq!(
            member.allows(permission),
            permission != Permission::MembersApprove,
            "{}",
            permission.name()
        );
    }
}
