use std::sync::mpsc::Sender;

use super::SystemAudioError;
use crate::shared::audio::CaptureEvent;

pub struct StubCapture;

impl StubCapture {
    pub fn start(_tx: Sender<CaptureEvent>) -> Result<Self, SystemAudioError> {
        Err(SystemAudioError::Unsupported(
            "pending platform support".to_string(),
        ))
    }

    pub fn stop(self) {}
}
