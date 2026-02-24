use super::{Frame, SidecarError};

pub struct JsonlCodec;

impl JsonlCodec {
    pub fn encode(frame: &Frame) -> Result<String, SidecarError> {
        let mut s =
            serde_json::to_string(frame).map_err(|e| SidecarError::Serialize(e.to_string()))?;
        s.push('\n');
        Ok(s)
    }

    pub fn decode(line: &str) -> Result<Frame, SidecarError> {
        serde_json::from_str(line).map_err(|e| SidecarError::Deserialize(e.to_string()))
    }
}
