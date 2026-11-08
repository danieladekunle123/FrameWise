//! Health score: 100 minus a penalty per open issue, weighted by impact.
//!
//! Choose items (security trade-offs such as Memory Integrity) never lower the score, so the score
//! can't push anyone into turning off a protection. Unknown and not-applicable checks don't count.

use crate::model::{Action, Finding, Impact, Status};

pub fn penalty(impact: Impact) -> u32 {
    match impact {
        Impact::High => 15,
        Impact::Medium => 8,
        Impact::LowMedium => 5,
        Impact::Low => 3,
    }
}

pub fn health_score(findings: &[Finding]) -> u8 {
    let total: u32 = findings
        .iter()
        .filter(|f| f.status == Status::Issue && f.action != Action::Choose)
        .map(|f| penalty(f.impact))
        .sum();
    100u32.saturating_sub(total) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{evaluate_all, Snapshot};

    #[test]
    fn empty_snapshot_scores_100() {
        // No data means no claimed issues, so nothing is penalised.
        assert_eq!(health_score(&evaluate_all(&Snapshot::default())), 100);
    }

    #[test]
    fn issues_lower_the_score_but_choices_do_not() {
        let mut snap = Snapshot::default();
        snap.os.build = 26100;
        snap.game_mode_auto = Some(0); // Low: -3
        snap.hvci_enabled = Some(1); // Choose: no penalty
        assert_eq!(health_score(&evaluate_all(&snap)), 97);
    }
}
