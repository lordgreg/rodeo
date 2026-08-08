//! uid/gid ↔ name lookups, read from `/etc/passwd` and `/etc/group`.
//!
//! Deliberately dependency-free: these are cosmetic (the owner column) or a
//! small convenience (typing a name instead of a number into the
//! permissions popup), not worth a crate. Each file is read once and cached;
//! users/groups that only exist in a directory service (LDAP/SSSD) are not
//! in either file, so callers fall back to the numeric id.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Parses `/etc/passwd` or `/etc/group`'s `name:x:id:...` shape into both
/// directions at once, so a single read serves uid→name and name→uid alike.
fn load_id_map(path: &str) -> (HashMap<u32, String>, HashMap<String, u32>) {
    let mut by_id = HashMap::new();
    let mut by_name = HashMap::new();

    let Ok(contents) = std::fs::read_to_string(path) else {
        return (by_id, by_name);
    };

    for line in contents.lines() {
        let mut fields = line.split(':');
        let (Some(name), Some(_), Some(id)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        let Ok(id) = id.parse::<u32>() else {
            continue;
        };
        by_id.entry(id).or_insert_with(|| name.to_string());
        by_name.entry(name.to_string()).or_insert(id);
    }

    (by_id, by_name)
}

fn users() -> &'static (HashMap<u32, String>, HashMap<String, u32>) {
    static USERS: OnceLock<(HashMap<u32, String>, HashMap<String, u32>)> = OnceLock::new();
    USERS.get_or_init(|| load_id_map("/etc/passwd"))
}

fn groups() -> &'static (HashMap<u32, String>, HashMap<String, u32>) {
    static GROUPS: OnceLock<(HashMap<u32, String>, HashMap<String, u32>)> = OnceLock::new();
    GROUPS.get_or_init(|| load_id_map("/etc/group"))
}

/// uid → user name.
pub fn user_name(uid: u32) -> Option<String> {
    users().0.get(&uid).cloned()
}

/// User name → uid.
pub fn name_to_uid(name: &str) -> Option<u32> {
    users().1.get(name).copied()
}

/// gid → group name.
pub fn group_name(gid: u32) -> Option<String> {
    groups().0.get(&gid).cloned()
}

/// Group name → gid.
pub fn name_to_gid(name: &str) -> Option<u32> {
    groups().1.get(name).copied()
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    #[test]
    fn root_resolves_both_ways() {
        // uid/gid 0 is "root" on every Unix system this runs on.
        assert_eq!(user_name(0), Some("root".to_string()));
        assert_eq!(name_to_uid("root"), Some(0));
    }

    #[test]
    fn an_unknown_id_resolves_to_nothing() {
        assert_eq!(user_name(u32::MAX), None);
        assert_eq!(name_to_uid("definitely-not-a-real-user"), None);
    }

    #[test]
    fn group_zero_is_root_or_wheel() {
        // Both names are used across Linux distros/macOS for gid 0.
        let name = group_name(0);
        assert!(
            matches!(name.as_deref(), Some("root") | Some("wheel")),
            "{name:?}"
        );
        assert_eq!(name.as_deref().and_then(name_to_gid), Some(0));
    }
}
