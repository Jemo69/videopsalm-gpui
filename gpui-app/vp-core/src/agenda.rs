//! Agenda document operations: add/insert/rename/replace-file/move/remove.
//!
//! SPEC 2.1: ordered list of heterogeneous items with per-item display name,
//! file ref OR embedded ref, flow type + auto-advance, verse/order
//! overrides; save/open/portable agenda file; reopen-recent (recents live
//! in [`crate::library`]).

use crate::schema::{default_flow_for, Agenda, AgendaItem, FlowType, ItemKind, ItemRef};

/// Generate a unique item id that is extremely unlikely to collide.
pub fn new_id(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Mix in the item count via process id for same-nanosecond calls.
    format!("{prefix}-{}-{}", std::process::id(), nanos)
}

/// Append an item and return its id.
pub fn add_item(
    agenda: &mut Agenda,
    kind: ItemKind,
    title: impl Into<String>,
    reference: ItemRef,
) -> String {
    let id = new_id("item");
    agenda.items.push(AgendaItem {
        id: id.clone(),
        kind,
        title: title.into(),
        reference,
        flow_type: default_flow_for(kind),
        auto_advance_ms: None,
        overrides: Default::default(),
    });
    id
}

/// Insert an item at `index` (clamped) and return its id.
pub fn insert_item(
    agenda: &mut Agenda,
    index: usize,
    kind: ItemKind,
    title: impl Into<String>,
    reference: ItemRef,
) -> String {
    let id = new_id("item");
    let index = index.min(agenda.items.len());
    agenda.items.insert(
        index,
        AgendaItem {
            id: id.clone(),
            kind,
            title: title.into(),
            reference,
            flow_type: default_flow_for(kind),
            auto_advance_ms: None,
            overrides: Default::default(),
        },
    );
    id
}

/// Rename an item's display name. Returns false when the id is unknown.
pub fn rename_item(agenda: &mut Agenda, id: &str, title: &str) -> bool {
    match agenda.items.iter_mut().find(|i| i.id == id) {
        Some(item) => {
            item.title = title.to_string();
            true
        }
        None => false,
    }
}

/// Replace the file/url/song/bible payload of an item.
pub fn replace_ref(agenda: &mut Agenda, id: &str, reference: ItemRef) -> bool {
    match agenda.items.iter_mut().find(|i| i.id == id) {
        Some(item) => {
            item.reference = reference;
            true
        }
        None => false,
    }
}

/// Move an item one slot up (toward the front). Returns false at the top.
pub fn move_up(agenda: &mut Agenda, id: &str) -> bool {
    match agenda.items.iter().position(|i| i.id == id) {
        Some(0) | None => false,
        Some(idx) => {
            agenda.items.swap(idx - 1, idx);
            true
        }
    }
}

/// Move an item one slot down (toward the back). Returns false at the end.
pub fn move_down(agenda: &mut Agenda, id: &str) -> bool {
    match agenda.items.iter().position(|i| i.id == id) {
        None => false,
        Some(idx) if idx + 1 >= agenda.items.len() => false,
        Some(idx) => {
            agenda.items.swap(idx, idx + 1);
            true
        }
    }
}

/// Remove an item by id. Returns false when the id is unknown.
pub fn remove_item(agenda: &mut Agenda, id: &str) -> bool {
    match agenda.items.iter().position(|i| i.id == id) {
        Some(idx) => {
            agenda.items.remove(idx);
            true
        }
        None => false,
    }
}

/// Set the flow type (+ optional auto-advance duration).
pub fn set_flow(
    agenda: &mut Agenda,
    id: &str,
    flow: FlowType,
    auto_advance_ms: Option<u64>,
) -> bool {
    match agenda.items.iter_mut().find(|i| i.id == id) {
        Some(item) => {
            item.flow_type = flow;
            item.auto_advance_ms = auto_advance_ms;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Agenda {
        let mut a = Agenda::new("Sunday");
        add_item(
            &mut a,
            ItemKind::Song,
            "Song",
            ItemRef::Song {
                song_id: "s1".into(),
            },
        );
        add_item(
            &mut a,
            ItemKind::Alert,
            "Alert",
            ItemRef::Text { text: "Hi".into() },
        );
        a
    }

    #[test]
    fn add_insert_rename_remove() {
        let mut a = sample();
        assert_eq!(a.items.len(), 2);
        let id = insert_item(
            &mut a,
            0,
            ItemKind::Clock,
            "Clock",
            ItemRef::Text { text: "c".into() },
        );
        assert_eq!(a.items[0].id, id);
        assert!(rename_item(&mut a, &id, "Countdown"));
        assert_eq!(a.items[0].title, "Countdown");
        assert!(!rename_item(&mut a, "nope", "x"));
        assert!(remove_item(&mut a, &id));
        assert_eq!(a.items.len(), 2);
        assert!(!remove_item(&mut a, &id));
    }

    #[test]
    fn move_up_down_with_bounds() {
        let mut a = sample();
        let first = a.items[0].id.clone();
        let second = a.items[1].id.clone();
        assert!(!move_up(&mut a, &first));
        assert!(move_down(&mut a, &first));
        assert_eq!(a.items[0].id, second);
        assert!(!move_down(&mut a, &first));
        assert!(move_up(&mut a, &first));
        assert_eq!(a.items[0].id, first);
        assert!(!move_up(&mut a, "missing"));
    }

    #[test]
    fn agenda_serde_round_trip_and_validate() {
        let mut a = sample();
        let first_id = a.items[0].id.clone();
        set_flow(&mut a, &first_id, FlowType::Auto, Some(5000));
        let json = a.to_json_pretty().unwrap();
        let back = Agenda::from_json(&json).unwrap();
        assert_eq!(a, back);
        assert!(back.validate().is_ok());
        let mut bad = back.clone();
        bad.version = 99;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn all_core_kinds_serialize() {
        // Acceptance: agenda with >=1 of each core kind round-trips.
        let mut a = Agenda::new("All kinds");
        for (kind, title) in [
            (ItemKind::Song, "song"),
            (ItemKind::BibleVerses, "bible"),
            (ItemKind::Image, "image"),
            (ItemKind::Video, "video"),
            (ItemKind::Audio, "audio"),
            (ItemKind::Pdf, "pdf"),
            (ItemKind::PowerPoint, "pptx"),
            (ItemKind::YouTube, "yt"),
            (ItemKind::Clock, "clock"),
            (ItemKind::Alert, "alert"),
        ] {
            add_item(
                &mut a,
                kind,
                title,
                ItemRef::File {
                    file: format!("{title}.dat"),
                },
            );
        }
        let json = a.to_json_pretty().unwrap();
        let back = Agenda::from_json(&json).unwrap();
        assert_eq!(back.items.len(), 10);
        assert_eq!(a, back);
    }
}
