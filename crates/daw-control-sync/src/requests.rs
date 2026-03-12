//! DAW request types for the synchronous queue

/// A single FX parameter change request
#[derive(Clone, Copy, Debug)]
pub struct FxParamRequest {
    pub track_idx: u32,
    pub fx_idx: u32,
    pub param_idx: u32,
    pub value: f32,
}

/// Enum of all possible DAW operations that can be queued
#[derive(Clone, Debug)]
pub enum DawRequest {
    /// Set an FX parameter value
    SetFxParam(FxParamRequest),

    /// Get an FX parameter value (read-only, for monitoring)
    GetFxParam {
        track_idx: u32,
        fx_idx: u32,
        param_idx: u32,
    },

    // Future request types can be added here:
    // SetTrackMute { track_idx: u32, muted: bool },
    // SetTrackVolume { track_idx: u32, volume: f32 },
    // SendMidiNote { track_idx: u32, note: u8, velocity: u8 },
    // PlayNote { duration_ms: u32 },
    // etc.
}

impl DawRequest {
    /// Get a human-readable description of this request
    pub fn description(&self) -> String {
        match self {
            DawRequest::SetFxParam(req) => {
                format!(
                    "SetFxParam(track={}, fx={}, param={}, value={})",
                    req.track_idx, req.fx_idx, req.param_idx, req.value
                )
            }
            DawRequest::GetFxParam {
                track_idx,
                fx_idx,
                param_idx,
            } => {
                format!(
                    "GetFxParam(track={}, fx={}, param={})",
                    track_idx, fx_idx, param_idx
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_description() {
        let req = DawRequest::SetFxParam(FxParamRequest {
            track_idx: 0,
            fx_idx: 1,
            param_idx: 2,
            value: 0.5,
        });
        let desc = req.description();
        assert!(desc.contains("SetFxParam"));
        assert!(desc.contains("track=0"));
        assert!(desc.contains("fx=1"));
    }
}
