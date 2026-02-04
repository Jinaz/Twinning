pub mod synchronizer {
    #[path = "synchronizer.rs"]
    pub mod synchronizer;

    pub mod mappings {
        #[path = "positionmapping.rs"]
        pub mod positionmapping;
    }
}

pub mod eventsystem {
    #[path = "eventqueue.rs"]
    pub mod eventqueue;

    #[path = "workflowengine.rs"]
    pub mod workflowengine;
}
