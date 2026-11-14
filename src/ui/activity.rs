//! Which program reports are news: a state a program just entered, and attention the user
//! hasn't seen yet. Reports re-read after a reconnect are adopted without raising anything.
use crate::rex::{
    activity::{ProgramState, StatusRecord},
    host::HostModel,
};
use std::collections::{HashMap, HashSet};

/// A pane, by its host's index and block ID.
pub type PaneKey = (usize, String);

#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    pub host: usize,
    pub session: String,
    pub block: String,
    pub record: StatusRecord,
}

#[derive(Default)]
pub struct Tracker {
    states: HashMap<PaneKey, ProgramState>,
    unseen: HashSet<PaneKey>,
    epochs: HashMap<usize, u64>,
}

fn needs_attention(state: ProgramState) -> bool {
    matches!(
        state,
        ProgramState::Blocked | ProgramState::Error | ProgramState::Done
    )
}

impl Tracker {
    /// Compare a host's model with the last one and return the states panes just entered.
    /// Panes entering a state that needs attention become unseen until `seen`.
    pub fn update(&mut self, host: usize, model: &HostModel) -> Vec<Transition> {
        let reloaded = self.epochs.insert(host, model.epoch) != Some(model.epoch);
        let mut transitions = Vec::new();
        let mut present = HashSet::new();
        for (block, state) in &model.blocks {
            let key = (host, block.clone());
            present.insert(key.clone());
            let record = state.status().cloned();
            let current = record.as_ref().map_or(ProgramState::Idle, |r| r.state);
            let previous = self.states.insert(key.clone(), current);
            if reloaded
                || previous == Some(current)
                || previous.is_none() && current == ProgramState::Idle
            {
                continue;
            }
            if needs_attention(current) {
                self.unseen.insert(key.clone());
            } else {
                self.unseen.remove(&key);
            }
            if let Some(record) = record {
                transitions.push(Transition {
                    host,
                    session: state.session_id.clone(),
                    block: block.clone(),
                    record,
                });
            }
        }
        // Closed panes leave nothing behind.
        self.states
            .retain(|key, _| key.0 != host || present.contains(key));
        self.unseen
            .retain(|key| key.0 != host || present.contains(key));
        transitions
    }

    /// The user is looking at this pane. Returns whether it held unseen attention.
    pub fn seen(&mut self, key: &PaneKey) -> bool {
        self.unseen.remove(key)
    }

    pub fn is_unseen(&self, key: &PaneKey) -> bool {
        self.unseen.contains(key)
    }

    pub fn unseen_count(&self) -> usize {
        self.unseen.len()
    }

    pub fn forget_host(&mut self, host: usize) {
        self.states.retain(|key, _| key.0 != host);
        self.unseen.retain(|key| key.0 != host);
        self.epochs.remove(&host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rex::host::BlockState;
    use std::collections::BTreeMap;

    fn model(epoch: u64, states: &[(&str, Option<ProgramState>)]) -> HostModel {
        let mut model = HostModel {
            epoch,
            ..Default::default()
        };
        for (block, state) in states {
            let mut records = BTreeMap::new();
            if let Some(state) = state {
                records.insert(
                    String::new(),
                    StatusRecord {
                        state: *state,
                        app: Some("claude".into()),
                        ..Default::default()
                    },
                );
            }
            model.blocks.insert(
                (*block).into(),
                BlockState {
                    session_id: "s".into(),
                    records,
                    ..Default::default()
                },
            );
        }
        model
    }

    #[test]
    fn new_states_are_news_and_reloads_are_not() {
        let mut tracker = Tracker::default();
        // The first sight of a host adopts what's there without notifying.
        let initial = tracker.update(
            0,
            &model(1, &[("a", Some(ProgramState::Blocked)), ("b", None)]),
        );
        assert!(initial.is_empty());
        assert_eq!(tracker.unseen_count(), 0);

        let working = tracker.update(
            0,
            &model(
                1,
                &[
                    ("a", Some(ProgramState::Blocked)),
                    ("b", Some(ProgramState::Working)),
                ],
            ),
        );
        assert_eq!(working.len(), 1);
        assert_eq!(working[0].record.state, ProgramState::Working);
        assert_eq!(tracker.unseen_count(), 0, "working needs no attention");

        let done = tracker.update(
            0,
            &model(
                1,
                &[
                    ("a", Some(ProgramState::Blocked)),
                    ("b", Some(ProgramState::Done)),
                ],
            ),
        );
        assert_eq!(done[0].block, "b");
        assert!(tracker.is_unseen(&(0, "b".into())));
        assert!(tracker.seen(&(0, "b".into())));
        assert!(!tracker.seen(&(0, "b".into())));

        // A reconnect re-reads every report; none of it is news.
        let reloaded = tracker.update(
            0,
            &model(
                2,
                &[
                    ("a", Some(ProgramState::Error)),
                    ("b", Some(ProgramState::Done)),
                ],
            ),
        );
        assert!(reloaded.is_empty());
    }

    #[test]
    fn closed_panes_and_hosts_are_forgotten() {
        let mut tracker = Tracker::default();
        tracker.update(0, &model(1, &[("a", None)]));
        tracker.update(0, &model(1, &[("a", Some(ProgramState::Error))]));
        assert_eq!(tracker.unseen_count(), 1);
        tracker.update(0, &model(1, &[]));
        assert_eq!(tracker.unseen_count(), 0);
        tracker.update(1, &model(1, &[("x", None)]));
        tracker.update(1, &model(1, &[("x", Some(ProgramState::Done))]));
        tracker.forget_host(1);
        assert_eq!(tracker.unseen_count(), 0);
    }
}
