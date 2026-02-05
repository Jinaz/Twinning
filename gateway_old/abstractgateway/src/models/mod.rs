pub mod abstractProperty;

#[derive(Debug, Default)]
pub struct PropertyModel {
    pub position: PositionProperty,
    pub speed: SpeedProperty,
}