//! The autoplay bot, over synthetic worlds.
use crate::autoplay::AutoplayAgent;

#[test]
fn an_agent_starts_idle() {
    let agent = AutoplayAgent::new(42, false).unwrap();
    assert_eq!(agent.status().actions, 0);
}
