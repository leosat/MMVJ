/// Macro to generate the MappedCtls enum and all its boilerplate conversions
#[macro_export]
macro_rules! app_ctl_types_to_platform_api {
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
        key {
            $( $kbd_variant:ident => $kbd_code:ident $(, $kbd_doc:literal)? ),* $(,)?
        }
        midi {
            $( $midi_variant:ident $(, $midi_doc:literal)? ),* $(,)?
        }
    ) => {
        /// Internal event codes enumeration, flat and abstracting from platform-related
        /// implementation, conversion from and to configuration string representation
        /// with strum.
        #[derive(Debug, PartialEq, EnumString, Display, Clone, Default, Copy, Hash, Eq, schemars::JsonSchema)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        #[schemars(rename_all = "SCREAMING_SNAKE_CASE")]
        #[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
        #[allow(nonstandard_style)]
        pub(crate) enum MappedCtls {
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
                $(#[doc = $kbd_doc])?
                $kbd_variant,
            )*
            $(
                $(#[doc = $midi_doc])?
                $midi_variant,
            )*
            // -- Special controls to read force feedback as a general device control input --
            #[strum(
                to_string = "FORCE_FEEDBACK_X",
                serialize = "ABS_SPECIAL_FORCE_FEEDBACK_X")]
            ForceFeedbackX,
            #[strum(
                to_string = "FORCE_FEEDBACK_Y",
                serialize = "ABS_SPECIAL_FORCE_FEEDBACK_Y")]
            ForceFeedbackY,
            #[default]
            Unhandled,
        }

        use strum_macros::EnumIter;
        #[derive(Debug, Serialize, Deserialize, PartialEq, EnumIter, EnumString, Display, Clone, Default, Copy, Hash, Eq, schemars::JsonSchema)]
        #[serde(rename_all = "snake_case")]
        #[strum(serialize_all = "snake_case")]
        #[schemars(rename_all = "snake_case")]
        pub(crate) enum MappedCtlsMidi {
            $(
                $(#[doc = $midi_doc])?
                $midi_variant,
            )*
            #[default]
            Unhandled,
        }

        impl MappedCtls {
            #[allow(dead_code)]
            pub(crate) fn is_special_force_feedback_x(&self) -> bool {
                *self == Self::ForceFeedbackX
            }

            #[allow(dead_code)]
            pub(crate) fn is_special_force_feedback_y(&self) -> bool {
                *self == Self::ForceFeedbackY
            }

            #[allow(dead_code)]
            pub(crate) fn is_unhandled(&self) -> bool {
                *self == Self::Unhandled
            }

            #[allow(dead_code)]
            pub(crate) fn get_relativity(&self) -> $crate::relativity::Relativity {
                self.is_relative().into()
            }

            #[allow(dead_code)]
            pub(crate) fn is_key(&self) -> bool {
                matches!(
                    self,
                    $( MappedCtls::$kbd_variant )|*
                )
            }

            #[allow(dead_code)]
            pub(crate) fn is_absolute(&self) -> bool {
                matches!(
                    self,
                    $( MappedCtls::$abs_variant )|*
                )
            }

            #[allow(dead_code)]
            pub(crate) fn is_relative(&self) -> bool {
                matches!(
                    self,
                    $( MappedCtls::$rel_variant )|*
                )
            }

            #[allow(dead_code)]
            pub(crate) fn is_button(&self) -> bool {
                matches!(
                    self,
                    $( MappedCtls::$btn_variant )|*
                )
            }

            /// Returns an iterator over all absolute control types
            #[allow(dead_code)]
            pub(crate) fn iter_absolute() -> AbsoluteMappedCtlsIter {
                AbsoluteMappedCtlsIter { index: 0 }
            }

            /// Returns an iterator over all relative control types
            #[allow(dead_code)]
            pub(crate) fn iter_relative() -> RelativeMappedCtlsIter {
                RelativeMappedCtlsIter { index: 0 }
            }

            /// Returns an iterator over all button control types
            #[allow(dead_code)]
            pub(crate) fn iter_button() -> ButtonMappedCtlsIter {
                ButtonMappedCtlsIter { index: 0 }
            }

            /// Returns an iterator over all key control types
            #[allow(dead_code)]
            pub(crate) fn iter_key() -> KeyMappedCtlsIter {
                KeyMappedCtlsIter { index: 0 }
            }
        }

        // Absolute controls iterator
        #[derive(Debug, Clone)]
        pub(crate) struct AbsoluteMappedCtlsIter {
            index: usize,
        }

        impl Iterator for AbsoluteMappedCtlsIter {
            type Item = MappedCtls;

            fn next(&mut self) -> Option<Self::Item> {
                let mut _current_index = 0;
                $(
                    if self.index == _current_index {
                        self.index += 1;
                        return Some(MappedCtls::$abs_variant);
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

        impl ExactSizeIterator for AbsoluteMappedCtlsIter {}

        // Relative controls iterator
        #[derive(Debug, Clone)]
        pub(crate) struct RelativeMappedCtlsIter {
            index: usize,
        }

        impl Iterator for RelativeMappedCtlsIter {
            type Item = MappedCtls;

            fn next(&mut self) -> Option<Self::Item> {
                let mut _current_index = 0;
                $(
                    if self.index == _current_index {
                        self.index += 1;
                        return Some(MappedCtls::$rel_variant);
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

        impl ExactSizeIterator for RelativeMappedCtlsIter {}

        // Button controls iterator
        #[derive(Debug, Clone)]
        pub(crate) struct ButtonMappedCtlsIter {
            index: usize,
        }

        impl Iterator for ButtonMappedCtlsIter {
            type Item = MappedCtls;

            fn next(&mut self) -> Option<Self::Item> {
                let mut _current_index = 0;
                $(
                    if self.index == _current_index {
                        self.index += 1;
                        return Some(MappedCtls::$btn_variant);
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

        impl ExactSizeIterator for ButtonMappedCtlsIter {}


        // Button controls iterator
        #[derive(Debug, Clone)]
        pub(crate) struct KeyMappedCtlsIter {
            index: usize,
        }

        impl Iterator for KeyMappedCtlsIter {
            type Item = MappedCtls;

            fn next(&mut self) -> Option<Self::Item> {
                let mut _current_index = 0;
                $(
                    if self.index == _current_index {
                        self.index += 1;
                        return Some(MappedCtls::$kbd_variant);
                    }
                    _current_index += 1;
                )*
                None
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = {
                    let total = [ $( stringify!($kbd_variant) ),* ].len();
                    total.saturating_sub(self.index)
                };
                (remaining, Some(remaining))
            }
        }

        impl ExactSizeIterator for KeyMappedCtlsIter {}


        impl From<evdev::InputEvent> for MappedCtls {
            fn from(event: evdev::InputEvent) -> Self {
                let code = event.code();
                type OsAbsAxisCode = evdev::AbsoluteAxisCode;
                type OsRelCode = evdev::RelativeAxisCode;
                type OsKeyCode = evdev::KeyCode;

                match event.event_type() {
                    evdev::EventType::ABSOLUTE => match code {
                        $(
                            c if c == OsAbsAxisCode::$abs_code.0 => MappedCtls::$abs_variant,
                        )*
                        c => {
                            log::warn!(
                                "Unimplemented handling for control {:?} \
                                while getting control type from {event:?}",
                                c
                            );
                            MappedCtls::Unhandled
                        }
                    },
                    evdev::EventType::RELATIVE => match code {
                        $(
                            c if c == OsRelCode::$rel_code.0 => MappedCtls::$rel_variant,
                        )*
                        c => {
                            log::warn!(
                                "Unimplemented handling for control {:?} \
                                while getting control type from {event:?}",
                                c
                            );
                            MappedCtls::Unhandled
                        }
                    },
                    evdev::EventType::KEY => match code {
                        $(
                            c if c == OsKeyCode::$btn_code.0 => MappedCtls::$btn_variant,
                        )*
                        $(
                            c if c == OsKeyCode::$kbd_code.0 => MappedCtls::$kbd_variant,
                        )*
                        c => {
                            log::warn!(
                                "Unimplemented handling for control {:?} \
                                while getting control type from {event:?}",
                                c
                            );
                            MappedCtls::Unhandled
                        }
                    },
                    _ => MappedCtls::Unhandled,
                }
            }
        }

        impl From<MappedCtls> for u16 {
            fn from(control_type: MappedCtls) -> Self {

                match control_type {
                    $(
                        MappedCtls::$abs_variant => OsAbsAxisCode::$abs_code.0,
                    )*
                    $(
                        MappedCtls::$rel_variant => OsRelCode::$rel_code.0,
                    )*
                    $(
                        MappedCtls::$btn_variant => OsKeyCode::$btn_code.0,
                    )*
                    $(
                        MappedCtls::$kbd_variant => OsKeyCode::$kbd_code.0,
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

        type OsAbsAxisCode = evdev::AbsoluteAxisCode;
        type OsRelCode = evdev::RelativeAxisCode;
        type OsKeyCode = evdev::KeyCode;

        impl TryFrom<OsAbsAxisCode> for MappedCtls {
            type Error = String;
            fn try_from(code: OsAbsAxisCode) -> std::result::Result<Self, Self::Error> {
                match code {
                    $(
                        c if c == OsAbsAxisCode::$abs_code => Ok(MappedCtls::$abs_variant),
                    )*
                    c => {
                        let err = format!("Unimplemented convesion from OS-specific abs axis code {:?} to internal MappedCtls",c);
                        log::warn!("{}", err);
                        Err(err)
                    }
                }
            }
        }

        impl Serialize for MappedCtls {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for MappedCtls {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<MappedCtls, D::Error> {
                let s = String::deserialize(deserializer)?;
                s.parse::<MappedCtls>()
                    .map_err(|_| de::Error::custom(format!("Invalid code string: {}", s)))
            }
        }
    };
}
