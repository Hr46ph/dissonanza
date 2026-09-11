mod core_bridge;

use core_bridge::BridgeCommand;
use dissonanza_core::roon::transport::{ChangeVolumeHow, MuteHow, SeekHow};

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let commands = core_bridge::spawn(ui.as_weak());

    // Phase 3.2: the transport bar's play/pause/prev/next buttons send `controlRequested`; unrecognized
    // action strings (shouldn't happen — `AppWindow.slint` only ever sends its own fixed vocabulary) are
    // dropped rather than sent as a malformed command.
    let control_commands = commands.clone();
    ui.on_controlRequested(move |zone_or_output_id, action| {
        if let Some(action) = core_bridge::parse_control_action(&action) {
            let _ = control_commands.send(BridgeCommand::Control {
                zone_or_output_id: zone_or_output_id.to_string(),
                action,
            });
        }
    });

    // Phase 3.2: clicking the seek bar sends `seekRequested` with an absolute target in seconds.
    let seek_commands = commands.clone();
    ui.on_seekRequested(move |zone_or_output_id, seconds| {
        let _ = seek_commands.send(BridgeCommand::Seek {
            zone_or_output_id: zone_or_output_id.to_string(),
            how: SeekHow::Absolute,
            seconds: seconds as f64,
        });
    });

    // Phase 3.3: the zone-switcher popup's "Pause all" row sends `pauseAllRequested`.
    let pause_all_commands = commands.clone();
    ui.on_pauseAllRequested(move || {
        let _ = pause_all_commands.send(BridgeCommand::PauseAll);
    });

    // Phase 3.4: the volume popover's slider sends an absolute target within the output's own range.
    let volume_commands = commands.clone();
    ui.on_volumeChangeRequested(move |output_id, value| {
        let _ = volume_commands.send(BridgeCommand::ChangeVolume {
            output_id: output_id.to_string(),
            how: ChangeVolumeHow::Absolute,
            value: value as f64,
        });
    });

    // Phase 3.4: the volume popover's mute toggle sends the target mute state directly.
    ui.on_muteToggleRequested(move |output_id, mute| {
        let how = if mute { MuteHow::Mute } else { MuteHow::Unmute };
        let _ = commands.send(BridgeCommand::Mute {
            output_id: output_id.to_string(),
            how,
        });
    });

    ui.run()
}
