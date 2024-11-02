use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
enum Messages {
    Status(Option<u32>),
}
