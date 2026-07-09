//! FTS transport action definitions shared across hosts/implementations.

actions_proto::define_actions! {
    pub fts_transport_actions {
        prefix: "fts.transport",
        title: crate::actions::daw_action_groups::TITLE,
        PLAY = "play" {
            name: "Play",
            description: "Start transport playback",
            category: Transport,
            group: crate::actions::GROUP_TRANSPORT,
        }
        PLAY_STOP = "play_stop" {
            name: "Play/Stop",
            description: "Toggle transport play/stop",
            category: Transport,
            group: crate::actions::GROUP_TRANSPORT,
        }
        PLAY_PAUSE = "play_pause" {
            name: "Play/Pause",
            description: "Toggle transport play/pause",
            category: Transport,
            group: crate::actions::GROUP_TRANSPORT,
        }
        PLAY_SKIP_TIME_SELECTION = "play_skip_time_selection" {
            name: "Play (Skip Time Selection)",
            description: "Start playback while skipping time selection",
            category: Transport,
            group: crate::actions::GROUP_TRANSPORT,
        }
        PLAY_FROM_LAST_START_POSITION = "play_from_last_start_position" {
            name: "Play From Last Start Position",
            description: "Start playback from the last position where play was started",
            category: Transport,
            group: crate::actions::GROUP_TRANSPORT,
        }
        TOGGLE_RECORDING = "toggle_recording" {
            name: "Toggle Recording",
            description: "Toggle transport recording state",
            category: Transport,
            group: crate::actions::GROUP_TRANSPORT,
        }
    }
}
