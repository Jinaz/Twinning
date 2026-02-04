pub mod databasemanager;
pub mod postgresclient;
pub mod postgresimpl;

pub mod models {
    pub mod postgrestypes;
    pub mod transformation;

    pub mod properties {
        pub mod position;
        pub mod speed;
    }
}
