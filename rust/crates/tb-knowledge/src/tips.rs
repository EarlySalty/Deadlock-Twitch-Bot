//! Gewichtet-abklingender Tipp-Ranker (rein, deterministisch, kein DB/Netz).
//! Höchster Score gewinnt den nächsten Go-Live-Slot.

use std::collections::HashMap;

use crate::doc::KnowledgeDoc;

/// Pro-Streamer-Zustand zu EINEM Doc (Slug). `None` = noch nie.
#[derive(Debug, Clone, Copy, Default)]
pub struct TipState {
    pub feature_used_days_ago: Option<i64>,
    pub tip_shown_days_ago: Option<i64>,
}

pub fn rank_tip<'a>(
    eligible: &[&'a KnowledgeDoc],
    state: &HashMap<String, TipState>,
) -> Option<&'a KnowledgeDoc> {
    eligible
        .iter()
        .copied()
        .map(|d| (score(d, state.get(&d.slug).copied().unwrap_or_default()), d))
        .max_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.time_to_value.cmp(&a.1.time_to_value))
                .then(b.1.slug.cmp(&a.1.slug))
        })
        .map(|(_, d)| d)
}

/// Gewichtet-abklingend. Defaults bewusst grob und später tunebar.
fn score(doc: &KnowledgeDoc, st: TipState) -> i64 {
    let mut s = (6 - doc.time_to_value.clamp(1, 5) as i64) * 10;

    match st.feature_used_days_ago {
        None => s += 50,
        Some(days) => s += days.clamp(0, 30),
    }

    if let Some(days) = st.tip_shown_days_ago {
        if days < 14 {
            s -= 40 - days.clamp(0, 14) * 2;
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::parse_doc;

    fn doc(slug: &str, ttv: u8) -> KnowledgeDoc {
        parse_doc(
            &format!(
                "---\ntitle: {slug}\nnamespace: bot\ntip_eligible: true\ntip_text: T\ntime_to_value: {ttv}\n---\nx"
            ),
            slug,
        )
        .unwrap()
    }

    #[test]
    fn nie_genutztes_feature_gewinnt_gegen_genutztes() {
        let a = doc("a", 3);
        let b = doc("b", 3);
        let docs = vec![&a, &b];
        let mut state = HashMap::new();
        state.insert(
            "b".to_string(),
            TipState {
                feature_used_days_ago: Some(0),
                tip_shown_days_ago: None,
            },
        );
        let pick = rank_tip(&docs, &state).unwrap();
        assert_eq!(pick.slug, "a", "unbenutztes Feature wird bevorzugt");
    }

    #[test]
    fn lange_nicht_genutzt_schlaegt_kuerzlich_genutzt() {
        let a = doc("a", 3);
        let b = doc("b", 3);
        let docs = vec![&a, &b];
        let mut state = HashMap::new();
        state.insert(
            "a".to_string(),
            TipState {
                feature_used_days_ago: Some(60),
                tip_shown_days_ago: None,
            },
        );
        state.insert(
            "b".to_string(),
            TipState {
                feature_used_days_ago: Some(1),
                tip_shown_days_ago: None,
            },
        );
        assert_eq!(
            rank_tip(&docs, &state).unwrap().slug,
            "a",
            "vergessenes Feature kommt als Reminder zurück"
        );
    }

    #[test]
    fn kuerzlich_gezeigter_tipp_wird_gedaempft() {
        let a = doc("a", 3);
        let b = doc("b", 3);
        let docs = vec![&a, &b];
        let mut state = HashMap::new();
        state.insert(
            "a".to_string(),
            TipState {
                feature_used_days_ago: None,
                tip_shown_days_ago: Some(0),
            },
        );
        assert_eq!(
            rank_tip(&docs, &state).unwrap().slug,
            "b",
            "nicht zweimal hintereinander derselbe Tipp"
        );
    }

    #[test]
    fn leere_liste_gibt_none() {
        assert!(rank_tip(&[], &HashMap::new()).is_none());
    }

    #[test]
    fn niedriges_ttv_gewinnt_bei_gleichstand() {
        let a = doc("a", 1);
        let b = doc("b", 5);
        let docs = vec![&a, &b];
        assert_eq!(rank_tip(&docs, &HashMap::new()).unwrap().slug, "a");
    }
}
