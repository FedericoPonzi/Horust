#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HorustMsgMessage {
    #[prost(oneof = "horust_msg_message::MessageType", tags = "1, 2")]
    pub message_type: ::core::option::Option<horust_msg_message::MessageType>,
}
/// Nested message and enum types in `HorustMsgMessage`.
pub mod horust_msg_message {
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum MessageType {
        #[prost(message, tag = "1")]
        Request(super::HorustMsgRequest),
        #[prost(message, tag = "2")]
        Response(super::HorustMsgResponse),
    }
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HorustMsgRequest {
    #[prost(oneof = "horust_msg_request::Request", tags = "1, 2")]
    pub request: ::core::option::Option<horust_msg_request::Request>,
}
/// Nested message and enum types in `HorustMsgRequest`.
pub mod horust_msg_request {
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Request {
        #[prost(message, tag = "1")]
        StatusRequest(super::HorustMsgServiceStatusRequest),
        #[prost(message, tag = "2")]
        ChangeRequest(super::HorustMsgServiceChangeRequest),
    }
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HorustMsgResponse {
    #[prost(oneof = "horust_msg_response::Response", tags = "1, 2")]
    pub response: ::core::option::Option<horust_msg_response::Response>,
}
/// Nested message and enum types in `HorustMsgResponse`.
pub mod horust_msg_response {
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Response {
        #[prost(message, tag = "1")]
        Error(super::HorustMsgError),
        #[prost(message, tag = "2")]
        StatusResponse(super::HorustMsgServiceStatusResponse),
    }
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HorustMsgError {
    #[prost(string, tag = "1")]
    pub error_string: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HorustMsgServiceStatusRequest {
    #[prost(string, tag = "1")]
    pub service_name: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HorustMsgServiceStatusResponse {
    #[prost(string, tag = "1")]
    pub service_name: ::prost::alloc::string::String,
    #[prost(enumeration = "HorustMsgServiceStatus", tag = "2")]
    pub service_status: i32,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HorustMsgServiceChangeRequest {
    #[prost(string, tag = "1")]
    pub service_name: ::prost::alloc::string::String,
    #[prost(enumeration = "HorustMsgServiceStatus", tag = "2")]
    pub service_status: i32,
}
/// return the current status - similar to HorustServiceStatusReponse.
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HorustMsgServiceChangeResponse {
    #[prost(string, tag = "1")]
    pub service_name: ::prost::alloc::string::String,
    #[prost(enumeration = "HorustMsgServiceStatus", tag = "2")]
    pub service_status: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum HorustMsgServiceStatus {
    Starting = 0,
    Started = 1,
    Running = 2,
    Inkilling = 3,
    Success = 4,
    Finished = 5,
    Finishedfailed = 6,
    Failed = 7,
    Initial = 8,
}
impl HorustMsgServiceStatus {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            HorustMsgServiceStatus::Starting => "STARTING",
            HorustMsgServiceStatus::Started => "STARTED",
            HorustMsgServiceStatus::Running => "RUNNING",
            HorustMsgServiceStatus::Inkilling => "INKILLING",
            HorustMsgServiceStatus::Success => "SUCCESS",
            HorustMsgServiceStatus::Finished => "FINISHED",
            HorustMsgServiceStatus::Finishedfailed => "FINISHEDFAILED",
            HorustMsgServiceStatus::Failed => "FAILED",
            HorustMsgServiceStatus::Initial => "INITIAL",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "STARTING" => Some(Self::Starting),
            "STARTED" => Some(Self::Started),
            "RUNNING" => Some(Self::Running),
            "INKILLING" => Some(Self::Inkilling),
            "SUCCESS" => Some(Self::Success),
            "FINISHED" => Some(Self::Finished),
            "FINISHEDFAILED" => Some(Self::Finishedfailed),
            "FAILED" => Some(Self::Failed),
            "INITIAL" => Some(Self::Initial),
            _ => None,
        }
    }
}
