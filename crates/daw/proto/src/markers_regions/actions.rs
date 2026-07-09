//! FTS marker/region action definitions shared across hosts/implementations.

actions_proto::define_actions! {
    pub fts_markers_regions_actions {
        prefix: "fts.markers_regions",
        title: crate::actions::daw_action_groups::TITLE,
        INSERT_REGION_AND_EDIT = "insert_region_and_edit" {
            name: "Insert Region and Edit",
            description: "Insert region from time selection and open editor",
            category: Project,
            group: crate::actions::GROUP_MARKERS_REGIONS,
        }
        INSERT_MARKER_AND_EDIT = "insert_marker_and_edit" {
            name: "Insert Marker and Edit",
            description: "Insert a new marker at cursor, then open marker editor",
            category: Project,
            group: crate::actions::GROUP_MARKERS_REGIONS,
        }
    }
}
