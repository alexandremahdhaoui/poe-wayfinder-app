use poe_wayfinder_core::controller::gamepad_match::{Chord, PadFamily, Reaction, Watcher};

use crate::adapter::gamepad_adapter::Gamepad;

#[cfg_attr(test, mockall::automock)]
pub trait PadInput {
    fn fired(&mut self) -> bool;

    fn connected(&self) -> bool;

    fn chord(&self) -> Option<Chord>;

    fn family(&self) -> PadFamily;

    fn held(&self) -> u16;

    fn rebind(&mut self, chord: Option<Chord>);
}

pub struct GamepadController {
    sources: Vec<Box<dyn Gamepad>>,
    watcher: Watcher,
    chord: Option<Chord>,
    connected: bool,
    family: PadFamily,
    held: u16,
}

impl GamepadController {
    pub fn new(sources: Vec<Box<dyn Gamepad>>, chord: Option<Chord>) -> Self {
        Self {
            sources,
            watcher: Watcher::new(chord.into_iter().collect()),
            chord,
            connected: false,
            family: PadFamily::default(),
            held: 0,
        }
    }

    fn read(&mut self) -> Option<u16> {
        let mut held: Option<u16> = None;
        let mut pushed = 0u16;
        let mut first_connected = None;
        let mut pressing = None;

        for source in &mut self.sources {
            let Some(buttons) = source.buttons() else {
                continue;
            };

            held = Some(held.unwrap_or(0) | buttons);
            pushed |= source.direction();
            first_connected = first_connected.or_else(|| Some(source.family()));

            if buttons != 0 && pressing.is_none() {
                pressing = Some(source.family());
            }
        }

        if let Some(family) = pressing.or(first_connected) {
            self.family = family;
        }

        self.held = held.unwrap_or(0) | pushed;

        held
    }
}

impl PadInput for GamepadController {
    fn fired(&mut self) -> bool {
        if self.chord.is_none() {
            return false;
        }

        let held = self.read();

        self.connected = held.is_some();

        matches!(self.watcher.react(held.unwrap_or(0)), Reaction::Fire { .. })
    }

    fn connected(&self) -> bool {
        self.connected
    }

    fn chord(&self) -> Option<Chord> {
        self.chord
    }

    fn family(&self) -> PadFamily {
        self.family
    }

    fn held(&self) -> u16 {
        self.held
    }

    fn rebind(&mut self, chord: Option<Chord>) {
        self.chord = chord;
        self.watcher = Watcher::new(chord.into_iter().collect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use poe_wayfinder_core::controller::gamepad_match::parse_chord;

    use crate::adapter::gamepad_adapter::MockGamepad;

    fn source(family: PadFamily, readings: Vec<Option<u16>>) -> Box<dyn Gamepad> {
        let mut pads = MockGamepad::new();
        let mut next = readings.into_iter();

        pads.expect_buttons()
            .returning(move || next.next().unwrap_or(None));
        pads.expect_family().return_const(family);
        pads.expect_direction().return_const(0u16);

        Box::new(pads)
    }

    fn chord() -> Option<Chord> {
        parse_chord("L1+R1+Triangle")
    }

    fn held() -> u16 {
        chord().expect("a chord").mask
    }

    #[test]
    fn no_chord_configured_never_reads_a_pad() {
        let mut pads = MockGamepad::new();

        pads.expect_buttons().never();
        pads.expect_family().return_const(PadFamily::Xbox);
        pads.expect_direction().return_const(0u16);

        let mut controller = GamepadController::new(vec![Box::new(pads)], None);

        assert!(!controller.fired());
        assert!(!controller.connected());
        assert_eq!(controller.chord(), None);
    }

    #[test]
    fn holding_the_chord_fires_once_and_not_on_every_poll() {
        let sources = vec![source(PadFamily::Xbox, vec![Some(held()), Some(held())])];
        let mut controller = GamepadController::new(sources, chord());

        assert!(controller.fired());
        assert!(!controller.fired());
    }

    #[test]
    fn letting_go_and_holding_it_again_fires_again() {
        let readings = vec![Some(held()), Some(0), Some(held())];
        let sources = vec![source(PadFamily::Xbox, readings)];
        let mut controller = GamepadController::new(sources, chord());

        assert!(controller.fired());
        assert!(!controller.fired());
        assert!(controller.fired());
    }

    #[test]
    fn a_playstation_pad_fires_a_chord_written_in_xbox_names() {
        let sources = vec![source(PadFamily::PlayStation, vec![Some(held())])];
        let mut controller = GamepadController::new(sources, parse_chord("LB+RB+Y"));

        assert!(controller.fired());
        assert_eq!(controller.family(), PadFamily::PlayStation);
    }

    #[test]
    fn either_pad_alone_fires_the_same_chord() {
        for family in [PadFamily::Xbox, PadFamily::PlayStation] {
            let sources = vec![source(family, vec![Some(held())])];
            let mut controller = GamepadController::new(sources, chord());

            assert!(controller.fired(), "{family:?}");
        }
    }

    #[test]
    fn two_pads_plugged_in_at_once_is_not_a_conflict() {
        let sources = vec![
            source(PadFamily::Xbox, vec![Some(0)]),
            source(PadFamily::PlayStation, vec![Some(held())]),
        ];
        let mut controller = GamepadController::new(sources, chord());

        assert!(controller.fired());
    }

    #[test]
    fn the_pad_being_pressed_is_the_one_the_status_row_names() {
        let sources = vec![
            source(PadFamily::Xbox, vec![Some(0)]),
            source(PadFamily::PlayStation, vec![Some(held())]),
        ];
        let mut controller = GamepadController::new(sources, chord());

        controller.fired();

        assert_eq!(controller.family(), PadFamily::PlayStation);
    }

    #[test]
    fn one_pad_holding_half_the_chord_does_not_fire_it_with_the_others_half() {
        let l1 = parse_chord("L1").expect("a chord").mask;
        let sources = vec![
            source(PadFamily::Xbox, vec![Some(l1)]),
            source(PadFamily::PlayStation, vec![Some(0)]),
        ];
        let mut controller = GamepadController::new(sources, chord());

        assert!(!controller.fired());
    }

    #[test]
    fn a_connected_pad_at_rest_is_connected_and_fires_nothing() {
        let sources = vec![source(PadFamily::PlayStation, vec![Some(0)])];
        let mut controller = GamepadController::new(sources, chord());

        assert!(!controller.fired());
        assert!(controller.connected());
        assert_eq!(controller.family(), PadFamily::PlayStation);
    }

    #[test]
    fn no_pad_on_any_source_reads_as_not_connected() {
        let sources = vec![
            source(PadFamily::Xbox, vec![None]),
            source(PadFamily::PlayStation, vec![None]),
        ];
        let mut controller = GamepadController::new(sources, chord());

        assert!(!controller.fired());
        assert!(!controller.connected());
    }

    #[test]
    fn the_buttons_held_right_now_are_reported_so_the_panel_can_be_navigated() {
        let sources = vec![source(PadFamily::PlayStation, vec![Some(held())])];
        let mut controller = GamepadController::new(sources, chord());

        controller.fired();

        assert_eq!(controller.held(), held());
    }

    #[test]
    fn a_stick_pushed_reads_as_held_so_the_panel_can_be_navigated_with_it() {
        let mut pads = MockGamepad::new();

        pads.expect_buttons().returning(|| Some(0));
        pads.expect_family().return_const(PadFamily::PlayStation);
        pads.expect_direction()
            .return_const(poe_wayfinder_core::controller::gamepad_match::DPAD_DOWN);

        let mut controller = GamepadController::new(vec![Box::new(pads)], chord());

        controller.fired();

        assert_eq!(
            controller.held(),
            poe_wayfinder_core::controller::gamepad_match::DPAD_DOWN
        );
    }

    #[test]
    fn a_rebound_chord_takes_effect_without_a_restart() {
        let old = parse_chord("L1+R1+Triangle");
        let fresh = parse_chord("Create+Options").expect("a chord");
        let sources = vec![source(PadFamily::PlayStation, vec![Some(fresh.mask)])];
        let mut controller = GamepadController::new(sources, old);

        controller.rebind(Some(fresh));

        assert_eq!(controller.chord(), Some(fresh));
        assert!(controller.fired(), "the new chord fires straight away");
    }

    #[test]
    fn rebinding_to_nothing_turns_the_pad_off() {
        let mut pads = MockGamepad::new();

        pads.expect_buttons().never();
        pads.expect_family().return_const(PadFamily::Xbox);
        pads.expect_direction().return_const(0u16);

        let mut controller = GamepadController::new(vec![Box::new(pads)], chord());

        controller.rebind(None);

        assert!(!controller.fired());
        assert_eq!(controller.chord(), None);
    }

    #[test]
    fn nothing_is_held_before_a_pad_has_been_read() {
        let controller = GamepadController::new(Vec::new(), chord());

        assert_eq!(controller.held(), 0);
    }

    #[test]
    fn the_chord_is_reported_so_the_status_window_can_name_it() {
        let controller = GamepadController::new(Vec::new(), parse_chord("BACK+L3"));

        assert_eq!(controller.chord(), parse_chord("BACK+L3"));
    }

    #[test]
    fn with_no_source_at_all_nothing_fires_and_nothing_panics() {
        let mut controller = GamepadController::new(Vec::new(), chord());

        assert!(!controller.fired());
        assert!(!controller.connected());
    }
}
