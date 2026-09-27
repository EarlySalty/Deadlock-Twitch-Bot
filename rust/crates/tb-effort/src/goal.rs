use crate::{Error, Result};

pub(crate) struct Route {
    pub points: i64,
    pub cap: i64,
    pub singular: String,
    pub plural: String,
    pub quest_reward: i64,
}

pub(crate) fn fastest(
    routes: &[Route],
    missing: i64,
    pending_quests: usize,
    completion_bonus: i64,
) -> Result<String> {
    if missing <= 0 {
        return Ok("Maximalstufe erreicht – dein Einsatz zählt weiter".into());
    }
    let rewardable: Vec<_> = routes
        .iter()
        .enumerate()
        .filter(|(_, r)| r.quest_reward > 0 && r.cap > 0)
        .map(|(i, _)| i)
        .collect();
    if rewardable.len() > 3 {
        return Err(Error::Invalid("quest_route_count"));
    }
    let mut order: Vec<_> = (0..routes.len()).collect();
    order.sort_by_key(|i| std::cmp::Reverse(routes[*i].points));
    let mut best: Option<(i64, Vec<i64>)> = None;
    for mask in 0usize..(1usize << rewardable.len()) {
        let mut counts = vec![0i64; routes.len()];
        let mut gain = 0i64;
        let mut completed = 0usize;
        for (bit, index) in rewardable.iter().enumerate() {
            if mask & (1usize << bit) != 0 {
                counts[*index] = 1;
                gain += routes[*index].points + routes[*index].quest_reward;
                completed += 1;
            }
        }
        if pending_quests > 0 && completed == pending_quests {
            gain += completion_bonus;
        }
        for index in &order {
            if gain >= missing {
                break;
            }
            let route = &routes[*index];
            if route.points <= 0 {
                continue;
            }
            let needed = (missing - gain + route.points - 1) / route.points;
            let take = needed.min(route.cap - counts[*index]);
            counts[*index] += take;
            gain += take * route.points;
        }
        let actions = counts.iter().sum();
        if gain >= missing && best.as_ref().is_none_or(|(n, _)| actions < *n) {
            best = Some((actions, counts));
        }
    }
    let (_, counts) = best.ok_or(Error::Invalid("unreachable_level"))?;
    let steps: Vec<_> = counts
        .into_iter()
        .zip(routes)
        .filter(|(n, _)| *n > 0)
        .map(|(n, r)| format!("{} {}", n, if n == 1 { &r.singular } else { &r.plural }))
        .collect();
    Ok(format!("Noch {}", steps.join(" und ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn route(points: i64, cap: i64, reward: i64, name: &str) -> Route {
        Route {
            points,
            cap,
            quest_reward: reward,
            singular: name.into(),
            plural: name.into(),
        }
    }
    #[test]
    fn rewards_and_remaining_caps_are_part_of_the_next_goal() {
        let routes = [route(50, i64::MAX, 0, "Partner"), route(2, 1, 100, "Clip")];
        assert_eq!(fastest(&routes, 80, 1, 10).unwrap(), "Noch 1 Clip");
        let routes = [route(50, i64::MAX, 0, "Partner"), route(100, 0, 0, "Match")];
        assert_eq!(fastest(&routes, 80, 0, 0).unwrap(), "Noch 2 Partner");
    }
    #[test]
    fn all_three_bonus_can_make_a_combination_shortest() {
        let routes = [
            route(10, i64::MAX, 5, "Einladung"),
            route(2, 2, 5, "Clip"),
            route(0, 1, 5, "Verlängerung"),
        ];
        assert_eq!(
            fastest(&routes, 37, 3, 10).unwrap(),
            "Noch 1 Einladung und 1 Clip und 1 Verlängerung"
        );
    }
}
