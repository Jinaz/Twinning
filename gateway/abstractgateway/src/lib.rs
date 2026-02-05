//! Digital Twin Gateway core library.
pub mod upstream;
pub mod downstream;
pub mod abstractgateway;

pub use upstream::iupstream::IUpstream;
pub use downstream::idownstream::IDownstream;
pub use abstractgateway::Gateway;
pub use abstractgateway::GatewayApi;
