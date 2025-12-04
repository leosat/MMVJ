/// Macro to generate the ControlType enum and all its boilerplate conversions
#[macro_export]
macro_rules! define_control_types {
    (
        absolute {
            $( $abs_variant:ident => $abs_code:ident $(, $abs_doc:literal)? ),* $(,)?
        }
        relative {
            $( $rel_variant:ident => $rel_code:ident $(, $rel_doc:literal)? ),* $(,)?
        }
        button {
            $( $btn_variant:ident => $btn_code:ident $(, $btn_doc:literal)? ),* $(,)?
        }
        midi {
            $( $midi_variant:ident $(, $midi_doc:literal)? ),* $(,)?
        }
    ) => {
        /// Internal event codes enumeration, flat and abstracting from platform-related
        /// implementation, conversion from and to configuration string representation
        /// with strum.
        #[derive(Debug, PartialEq, EnumString, Display, Clone, Default, Copy, Hash, Eq)]
        #[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
        pub(crate) enum ControlType {
            $(
                $(#[doc = $abs_doc])?
                $abs_variant,
            )*
            $(
                $(#[doc = $rel_doc])?
                $rel_variant,
            )*
            $(
                $(#[doc = $btn_doc])?
                $btn_variant,
            )*
            $(
                $(#[doc = $midi_doc])?
                $midi_variant,
            )*
            #[default]
            Unhandled,
        }

        impl ControlType {
            #[allow(dead_code)]
            pub(crate) fn is_unhandled(&self) -> bool {
                *self == Self::Unhandled
            }

            #[allow(dead_code)]
            pub(crate) fn is_absolute(&self) -> bool {
                matches!(
                    self,
                    $( ControlType::$abs_variant )|*
                )
            }

            #[allow(dead_code)]
            pub(crate) fn is_relative(&self) -> bool {
                matches!(
                    self,
                    $( ControlType::$rel_variant )|*
                )
            }

            #[allow(dead_code)]
            pub(crate) fn is_button(&self) -> bool {
                matches!(
                    self,
                    $( ControlType::$btn_variant )|*
                )
            }

            /// Returns an iterator over all absolute control types
            #[allow(dead_code)]
            pub(crate) fn iter_absolute() -> AbsoluteControlTypeIter {
                AbsoluteControlTypeIter { index: 0 }
            }

            /// Returns an iterator over all relative control types
            #[allow(dead_code)]
            pub(crate) fn iter_relative() -> RelativeControlTypeIter {
                RelativeControlTypeIter { index: 0 }
            }

            /// Returns an iterator over all button control types
            #[allow(dead_code)]
            pub(crate) fn iter_button() -> ButtonControlTypeIter {
                ButtonControlTypeIter { index: 0 }
            }
        }

        // Absolute controls iterator
        #[derive(Debug, Clone)]
        pub(crate) struct AbsoluteControlTypeIter {
            index: usize,
        }

        impl Iterator for AbsoluteControlTypeIter {
            type Item = ControlType;

            fn next(&mut self) -> Option<Self::Item> {
                let mut _current_index = 0;
                $(
                    if self.index == _current_index {
                        self.index += 1;
                        return Some(ControlType::$abs_variant);
                    }
                    _current_index += 1;
                )*
                None
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = {
                    let total = [ $( stringify!($abs_variant) ),* ].len();
                    total.saturating_sub(self.index)
                };
                (remaining, Some(remaining))
            }
        }

        impl ExactSizeIterator for AbsoluteControlTypeIter {}

        // Relative controls iterator
        #[derive(Debug, Clone)]
        pub(crate) struct RelativeControlTypeIter {
            index: usize,
        }

        impl Iterator for RelativeControlTypeIter {
            type Item = ControlType;

            fn next(&mut self) -> Option<Self::Item> {
                let mut _current_index = 0;
                $(
                    if self.index == _current_index {
                        self.index += 1;
                        return Some(ControlType::$rel_variant);
                    }
                    _current_index += 1;
                )*
                None
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = {
                    let total = [ $( stringify!($rel_variant) ),* ].len();
                    total.saturating_sub(self.index)
                };
                (remaining, Some(remaining))
            }
        }

        impl ExactSizeIterator for RelativeControlTypeIter {}

        // Button controls iterator
        #[derive(Debug, Clone)]
        pub(crate) struct ButtonControlTypeIter {
            index: usize,
        }

        impl Iterator for ButtonControlTypeIter {
            type Item = ControlType;

            fn next(&mut self) -> Option<Self::Item> {
                let mut _current_index = 0;
                $(
                    if self.index == _current_index {
                        self.index += 1;
                        return Some(ControlType::$btn_variant);
                    }
                    _current_index += 1;
                )*
                None
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = {
                    let total = [ $( stringify!($btn_variant) ),* ].len();
                    total.saturating_sub(self.index)
                };
                (remaining, Some(remaining))
            }
        }

        impl ExactSizeIterator for ButtonControlTypeIter {}

        impl From<evdev::InputEvent> for ControlType {
            fn from(event: evdev::InputEvent) -> Self {
                let code = event.code();
                type OsAbsAxisCode = evdev::AbsoluteAxisCode;
                type OsRelCode = evdev::RelativeAxisCode;
                type OsKeyCode = evdev::KeyCode;

                match event.event_type() {
                    evdev::EventType::ABSOLUTE => match code {
                        $(
                            c if c == OsAbsAxisCode::$abs_code.0 => ControlType::$abs_variant,
                        )*
                        c => {
                            log::warn!(
                                "Unimplemented handling for control {:?} \
                                while getting control type from {event:?}",
                                c
                            );
                            ControlType::Unhandled
                        }
                    },
                    evdev::EventType::RELATIVE => match code {
                        $(
                            c if c == OsRelCode::$rel_code.0 => ControlType::$rel_variant,
                        )*
                        c => {
                            log::warn!(
                                "Unimplemented handling for control {:?} \
                                while getting control type from {event:?}",
                                c
                            );
                            ControlType::Unhandled
                        }
                    },
                    evdev::EventType::KEY => match code {
                        $(
                            c if c == OsKeyCode::$btn_code.0 => ControlType::$btn_variant,
                        )*
                        c => {
                            log::warn!(
                                "Unimplemented handling for control {:?} \
                                while getting control type from {event:?}",
                                c
                            );
                            ControlType::Unhandled
                        }
                    },
                    _ => ControlType::Unhandled,
                }
            }
        }

        impl From<ControlType> for u16 {
            fn from(control_type: ControlType) -> Self {
                type OsAbsAxisCode = evdev::AbsoluteAxisCode;
                type OsRelCode = evdev::RelativeAxisCode;
                type OsKeyCode = evdev::KeyCode;

                match control_type {
                    $(
                        ControlType::$abs_variant => OsAbsAxisCode::$abs_code.0,
                    )*
                    $(
                        ControlType::$rel_variant => OsRelCode::$rel_code.0,
                    )*
                    $(
                        ControlType::$btn_variant => OsKeyCode::$btn_code.0,
                    )*
                    c => {
                        log::warn!(
                            "Unimplemented handling for control type {:?} while converting to evdev code.",
                            c
                        );
                        0
                    }
                }
            }
        }

        impl Serialize for ControlType {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for ControlType {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ControlType, D::Error> {
                let s = String::deserialize(deserializer)?;
                s.parse::<ControlType>()
                    .map_err(|_| de::Error::custom(format!("Invalid code string: {}", s)))
            }
        }
    };
}
