use super::*;

/// A turn that ends in metadata still waiting for its surface is a turn.
#[test]
fn staged_metadata_reports_a_finished_turn() {
    use AgentStatus::*;
    use herdr_client::{
        ClientEvent,
        protocol::{FrameData, PaneSurfaceFrame},
    };
    let displayed = with([Working, Idle]);
    let mut live = crate::state::LiveState::default();
    live.apply(ClientEvent::Snapshot(displayed.clone()));
    live.apply(ClientEvent::Surface(Arc::new(PaneSurfaceFrame {
        boot_id: displayed.boot_id.clone(),
        projection_revision: displayed.revision,
        surface_revision: 1,
        frame: FrameData {
            cells: vec![],
            width: 0,
            height: 0,
            cursor: None,
            hyperlinks: vec![],
            graphics: vec![],
        },
        panes: vec![],
        splits: vec![],
        popup: None,
        graphics: Default::default(),
    })));
    let mut done = (*with([Done, Idle])).clone();
    done.revision += 1;
    live.apply(ClientEvent::Snapshot(Arc::new(done)));
    assert!(Arc::ptr_eq(live.snapshot.as_ref().unwrap(), &displayed));
    let mut checkpoints = Checkpoints::default();
    checkpoints.observe("local", &Host::Local, &displayed);
    checkpoints.observe("local", &Host::Local, live.latest_snapshot().unwrap());
    assert_eq!(
        queued(&checkpoints),
        [(feature(), capture("Claude Code finished a turn"))]
    );
}
