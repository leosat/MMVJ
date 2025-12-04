use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

use crate::base_num::BaseNumT;
use crate::{hid_device::HID_AXIS_MAX_INTERVAL, num_interval::NumInterval};

#[cfg(feature = "midi")]
use crate::midi::MIDIv1_PITCH_WHEEL_INTERVAL;
#[cfg(feature = "midi")]
use crate::schemas_midi::MidiControlMatcherCfg;
#[cfg(feature = "midi")]
use crate::schemas_midi::MidiMessageType;
#[cfg(feature = "midi")]
use crate::schemas_midi::MidiNumberCfg;

use serde::{Deserializer, Serializer, de};
use std::convert::*;
use strum_macros::{Display, EnumString};

use crate::{schemas_control_matcher::ControlMatchers, schemas_hid::HidControlMatcherCfg};

#[test]
fn dbg_control_names() {
    dbg!(MappedCtls::ForceFeedbackX.to_string());
    dbg!(MappedCtls::ForceFeedbackY.to_string());
}

app_ctl_types_to_platform_api! {
    absolute {
        // ---
        AbsX => ABS_X,
        AbsY => ABS_Y,
        AbsZ => ABS_Z,
        AbsRx => ABS_RX,
        AbsRy => ABS_RY,
        AbsRz => ABS_RZ,
        AbsThrottle => ABS_THROTTLE,
        AbsRudder => ABS_RUDDER,
        AbsWheel => ABS_WHEEL,
        AbsGas => ABS_GAS,
        AbsBrake => ABS_BRAKE,
        AbsHat0X => ABS_HAT0X,
        AbsHat0Y => ABS_HAT0Y,
        AbsHat1X => ABS_HAT1X,
        AbsHat1Y => ABS_HAT1Y,
        AbsHat2X => ABS_HAT2X,
        AbsHat2Y => ABS_HAT2Y,
        AbsHat3X => ABS_HAT3X,
        AbsHat3Y => ABS_HAT3Y,
        AbsPressure => ABS_PRESSURE,
        AbsDistance => ABS_DISTANCE,
        AbsTiltX => ABS_TILT_X,
        AbsTiltY => ABS_TILT_Y,
        AbsToolWidth => ABS_TOOL_WIDTH,
        AbsVolume => ABS_VOLUME,
        AbsMisc => ABS_MISC,
        AbsMtSlot => ABS_MT_SLOT, "MT slot being modified",
        AbsMtTouchMajor => ABS_MT_TOUCH_MAJOR, "Major axis of touching ellipse",
        AbsMtTouchMinor => ABS_MT_TOUCH_MINOR, "Minor axis (omit if circular)",
        AbsMtWidthMajor => ABS_MT_WIDTH_MAJOR, "Major axis of approaching ellipse",
        AbsMtWidthMinor => ABS_MT_WIDTH_MINOR, "Minor axis (omit if circular)",
        AbsMtOrientation => ABS_MT_ORIENTATION, "Ellipse orientation",
        AbsMtPositionX => ABS_MT_POSITION_X, "Center X touch position",
        AbsMtPositionY => ABS_MT_POSITION_Y, "Center Y touch position",
        AbsMtToolType => ABS_MT_TOOL_TYPE, "Type of touching device",
        AbsMtBlobId => ABS_MT_BLOB_ID, "Group a set of packets as a blob",
        AbsMtTrackingId => ABS_MT_TRACKING_ID, "Unique ID of the initiated contact",
        AbsMtPressure => ABS_MT_PRESSURE, "Pressure on contact area",
        AbsMtDistance => ABS_MT_DISTANCE, "Contact over distance",
        AbsMtToolX => ABS_MT_TOOL_X, "Center X tool position",
        AbsMtToolY => ABS_MT_TOOL_Y, "Center Y tool position",
    }
    relative {
        RelX => REL_X,
        RelY => REL_Y,
        RelZ => REL_Z,
        RelRX => REL_RX,
        RelRY => REL_RY,
        RelRZ => REL_RZ,
        RelHwheel => REL_HWHEEL,
        RelDial => REL_DIAL,
        RelWheel => REL_WHEEL,
        RelReserved => REL_RESERVED,
        RelMisc => REL_MISC,
        RelWheelHiRes => REL_WHEEL_HI_RES,
        RelHwheelHiRes => REL_HWHEEL_HI_RES,
    }
    button {
        Btn_0  =>     BTN_0 ,
        Btn_1  =>     BTN_1 ,
        Btn_2  =>     BTN_2 ,
        Btn_3  =>     BTN_3 ,
        Btn_4  =>     BTN_4 ,
        Btn_5  =>     BTN_5 ,
        Btn_6  =>     BTN_6 ,
        Btn_7  =>     BTN_7 ,
        Btn_8  =>     BTN_8 ,
        Btn_9  =>     BTN_9 ,
        BtnLeft  =>     BTN_LEFT ,
        BtnRight  =>     BTN_RIGHT ,
        BtnMiddle  =>     BTN_MIDDLE ,
        BtnSide  =>     BTN_SIDE ,
        BtnExtra  =>     BTN_EXTRA ,
        BtnForward  =>     BTN_FORWARD ,
        BtnBack  =>     BTN_BACK ,
        BtnTask  =>     BTN_TASK ,
        BtnTrigger  =>     BTN_TRIGGER ,
        BtnThumb  =>     BTN_THUMB ,
        BtnThumb2  =>     BTN_THUMB2 ,
        BtnTop  =>     BTN_TOP ,
        BtnTop2  =>     BTN_TOP2 ,
        BtnPinkie  =>     BTN_PINKIE ,
        BtnBase  =>     BTN_BASE ,
        BtnBase2  =>     BTN_BASE2 ,
        BtnBase3  =>     BTN_BASE3 ,
        BtnBase4  =>     BTN_BASE4 ,
        BtnBase5  =>     BTN_BASE5 ,
        BtnBase6  =>     BTN_BASE6 ,
        BtnDead  =>     BTN_DEAD ,
        BtnSouth  =>     BTN_SOUTH ,
        BtnEast  =>     BTN_EAST ,
        BtnC  =>     BTN_C ,
        BtnNorth  =>     BTN_NORTH ,
        BtnWest  =>     BTN_WEST ,
        BtnZ  =>     BTN_Z ,
        BtnTl  =>     BTN_TL ,
        BtnTr  =>     BTN_TR ,
        BtnTl2  =>     BTN_TL2 ,
        BtnTr2  =>     BTN_TR2 ,
        BtnSelect  =>     BTN_SELECT ,
        BtnStart  =>     BTN_START ,
        BtnMode  =>     BTN_MODE ,
        BtnThumbl  =>     BTN_THUMBL ,
        BtnThumbr  =>     BTN_THUMBR ,
        BtnToolPen  =>     BTN_TOOL_PEN ,
        BtnToolRubber  =>     BTN_TOOL_RUBBER ,
        BtnToolBrush  =>     BTN_TOOL_BRUSH ,
        BtnToolPencil  =>     BTN_TOOL_PENCIL ,
        BtnToolAirbrush  =>     BTN_TOOL_AIRBRUSH ,
        BtnToolFinger  =>     BTN_TOOL_FINGER ,
        BtnToolMouse  =>     BTN_TOOL_MOUSE ,
        BtnToolLens  =>     BTN_TOOL_LENS ,
        BtnToolQuinttap  =>     BTN_TOOL_QUINTTAP ,
        BtnTouch  =>     BTN_TOUCH ,
        BtnStylus  =>     BTN_STYLUS ,
        BtnStylus2  =>     BTN_STYLUS2 ,
        BtnToolDoubletap  =>     BTN_TOOL_DOUBLETAP ,
        BtnToolTripletap  =>     BTN_TOOL_TRIPLETAP ,
        BtnToolQuadtap  =>     BTN_TOOL_QUADTAP ,
        BtnGearDown  =>     BTN_GEAR_DOWN ,
        BtnGearUp  =>     BTN_GEAR_UP ,
        BtnDpadUp  =>     BTN_DPAD_UP ,
        BtnDpadDown  =>     BTN_DPAD_DOWN ,
        BtnDpadLeft  =>     BTN_DPAD_LEFT ,
        BtnDpadRight  =>     BTN_DPAD_RIGHT ,
        BtnTriggerHappy1  =>     BTN_TRIGGER_HAPPY1 ,
        BtnTriggerHappy2  =>     BTN_TRIGGER_HAPPY2 ,
        BtnTriggerHappy3  =>     BTN_TRIGGER_HAPPY3 ,
        BtnTriggerHappy4  =>     BTN_TRIGGER_HAPPY4 ,
        BtnTriggerHappy5  =>     BTN_TRIGGER_HAPPY5 ,
        BtnTriggerHappy6  =>     BTN_TRIGGER_HAPPY6 ,
        BtnTriggerHappy7  =>     BTN_TRIGGER_HAPPY7 ,
        BtnTriggerHappy8  =>     BTN_TRIGGER_HAPPY8 ,
        BtnTriggerHappy9  =>     BTN_TRIGGER_HAPPY9 ,
        BtnTriggerHappy10  =>     BTN_TRIGGER_HAPPY10 ,
        BtnTriggerHappy11  =>     BTN_TRIGGER_HAPPY11 ,
        BtnTriggerHappy12  =>     BTN_TRIGGER_HAPPY12 ,
        BtnTriggerHappy13  =>     BTN_TRIGGER_HAPPY13 ,
        BtnTriggerHappy14  =>     BTN_TRIGGER_HAPPY14 ,
        BtnTriggerHappy15  =>     BTN_TRIGGER_HAPPY15 ,
        BtnTriggerHappy16  =>     BTN_TRIGGER_HAPPY16 ,
        BtnTriggerHappy17  =>     BTN_TRIGGER_HAPPY17 ,
        BtnTriggerHappy18  =>     BTN_TRIGGER_HAPPY18 ,
        BtnTriggerHappy19  =>     BTN_TRIGGER_HAPPY19 ,
        BtnTriggerHappy20  =>     BTN_TRIGGER_HAPPY20 ,
        BtnTriggerHappy21  =>     BTN_TRIGGER_HAPPY21 ,
        BtnTriggerHappy22  =>     BTN_TRIGGER_HAPPY22 ,
        BtnTriggerHappy23  =>     BTN_TRIGGER_HAPPY23 ,
        BtnTriggerHappy24  =>     BTN_TRIGGER_HAPPY24 ,
        BtnTriggerHappy25  =>     BTN_TRIGGER_HAPPY25 ,
        BtnTriggerHappy26  =>     BTN_TRIGGER_HAPPY26 ,
        BtnTriggerHappy27  =>     BTN_TRIGGER_HAPPY27 ,
        BtnTriggerHappy28  =>     BTN_TRIGGER_HAPPY28 ,
        BtnTriggerHappy29  =>     BTN_TRIGGER_HAPPY29 ,
        BtnTriggerHappy30  =>     BTN_TRIGGER_HAPPY30 ,
        BtnTriggerHappy31  =>     BTN_TRIGGER_HAPPY31 ,
        BtnTriggerHappy32  =>     BTN_TRIGGER_HAPPY32 ,
        BtnTriggerHappy33  =>     BTN_TRIGGER_HAPPY33 ,
        BtnTriggerHappy34  =>     BTN_TRIGGER_HAPPY34 ,
        BtnTriggerHappy35  =>     BTN_TRIGGER_HAPPY35 ,
        BtnTriggerHappy36  =>     BTN_TRIGGER_HAPPY36 ,
        BtnTriggerHappy37  =>     BTN_TRIGGER_HAPPY37 ,
        BtnTriggerHappy38  =>     BTN_TRIGGER_HAPPY38 ,
        BtnTriggerHappy39  =>     BTN_TRIGGER_HAPPY39 ,
        BtnTriggerHappy40  =>     BTN_TRIGGER_HAPPY40 ,
        // --------------------------------------------
        BtnJoystick  =>     BTN_TRIGGER ,
        BtnGamepad  =>     BTN_SOUTH ,
    }
    key {
        KeyReserved  =>     KEY_RESERVED ,
        KeyEsc  =>     KEY_ESC ,
        Key_1  =>     KEY_1 ,
        Key_2  =>     KEY_2 ,
        Key_3  =>     KEY_3 ,
        Key_4  =>     KEY_4 ,
        Key_5  =>     KEY_5 ,
        Key_6  =>     KEY_6 ,
        Key_7  =>     KEY_7 ,
        Key_8  =>     KEY_8 ,
        Key_9  =>     KEY_9 ,
        Key_0  =>     KEY_0 ,
        KeyMinus  =>     KEY_MINUS ,
        KeyEqual  =>     KEY_EQUAL ,
        KeyBackspace  =>     KEY_BACKSPACE ,
        KeyTab  =>     KEY_TAB ,
        KeyQ  =>     KEY_Q ,
        KeyW  =>     KEY_W ,
        KeyE  =>     KEY_E ,
        KeyR  =>     KEY_R ,
        KeyT  =>     KEY_T ,
        KeyY  =>     KEY_Y ,
        KeyU  =>     KEY_U ,
        KeyI  =>     KEY_I ,
        KeyO  =>     KEY_O ,
        KeyP  =>     KEY_P ,
        KeyLeftbrace  =>     KEY_LEFTBRACE ,
        KeyRightbrace  =>     KEY_RIGHTBRACE ,
        KeyEnter  =>     KEY_ENTER ,
        KeyLeftctrl  =>     KEY_LEFTCTRL ,
        KeyA  =>     KEY_A ,
        KeyS  =>     KEY_S ,
        KeyD  =>     KEY_D ,
        KeyF  =>     KEY_F ,
        KeyG  =>     KEY_G ,
        KeyH  =>     KEY_H ,
        KeyJ  =>     KEY_J ,
        KeyK  =>     KEY_K ,
        KeyL  =>     KEY_L ,
        KeySemicolon  =>     KEY_SEMICOLON ,
        KeyApostrophe  =>     KEY_APOSTROPHE ,
        KeyGrave  =>     KEY_GRAVE ,
        KeyLeftshift  =>     KEY_LEFTSHIFT ,
        KeyBackslash  =>     KEY_BACKSLASH ,
        KeyZ  =>     KEY_Z ,
        KeyX  =>     KEY_X ,
        KeyC  =>     KEY_C ,
        KeyV  =>     KEY_V ,
        KeyB  =>     KEY_B ,
        KeyN  =>     KEY_N ,
        KeyM  =>     KEY_M ,
        KeyComma  =>     KEY_COMMA ,
        KeyDot  =>     KEY_DOT ,
        KeySlash  =>     KEY_SLASH ,
        KeyRightshift  =>     KEY_RIGHTSHIFT ,
        KeyKpasterisk  =>     KEY_KPASTERISK ,
        KeyLeftalt  =>     KEY_LEFTALT ,
        KeySpace  =>     KEY_SPACE ,
        KeyCapslock  =>     KEY_CAPSLOCK ,
        KeyF1  =>     KEY_F1 ,
        KeyF2  =>     KEY_F2 ,
        KeyF3  =>     KEY_F3 ,
        KeyF4  =>     KEY_F4 ,
        KeyF5  =>     KEY_F5 ,
        KeyF6  =>     KEY_F6 ,
        KeyF7  =>     KEY_F7 ,
        KeyF8  =>     KEY_F8 ,
        KeyF9  =>     KEY_F9 ,
        KeyF10  =>     KEY_F10 ,
        KeyNumlock  =>     KEY_NUMLOCK ,
        KeyScrolllock  =>     KEY_SCROLLLOCK ,
        KeyKp7  =>     KEY_KP7 ,
        KeyKp8  =>     KEY_KP8 ,
        KeyKp9  =>     KEY_KP9 ,
        KeyKpminus  =>     KEY_KPMINUS ,
        KeyKp4  =>     KEY_KP4 ,
        KeyKp5  =>     KEY_KP5 ,
        KeyKp6  =>     KEY_KP6 ,
        KeyKpplus  =>     KEY_KPPLUS ,
        KeyKp1  =>     KEY_KP1 ,
        KeyKp2  =>     KEY_KP2 ,
        KeyKp3  =>     KEY_KP3 ,
        KeyKp0  =>     KEY_KP0 ,
        KeyKpdot  =>     KEY_KPDOT ,
        KeyZenkakuhankaku  =>     KEY_ZENKAKUHANKAKU ,
        Key_102nd  =>     KEY_102ND ,
        KeyF11  =>     KEY_F11 ,
        KeyF12  =>     KEY_F12 ,
        KeyRo  =>     KEY_RO ,
        KeyKatakana  =>     KEY_KATAKANA ,
        KeyHiragana  =>     KEY_HIRAGANA ,
        KeyHenkan  =>     KEY_HENKAN ,
        KeyKatakanahiragana  =>     KEY_KATAKANAHIRAGANA ,
        KeyMuhenkan  =>     KEY_MUHENKAN ,
        KeyKpjpcomma  =>     KEY_KPJPCOMMA ,
        KeyKpenter  =>     KEY_KPENTER ,
        KeyRightctrl  =>     KEY_RIGHTCTRL ,
        KeyKpslash  =>     KEY_KPSLASH ,
        KeySysrq  =>     KEY_SYSRQ ,
        KeyRightalt  =>     KEY_RIGHTALT ,
        KeyLinefeed  =>     KEY_LINEFEED ,
        KeyHome  =>     KEY_HOME ,
        KeyUp  =>     KEY_UP ,
        KeyPageup  =>     KEY_PAGEUP ,
        KeyLeft  =>     KEY_LEFT ,
        KeyRight  =>     KEY_RIGHT ,
        KeyEnd  =>     KEY_END ,
        KeyDown  =>     KEY_DOWN ,
        KeyPagedown  =>     KEY_PAGEDOWN ,
        KeyInsert  =>     KEY_INSERT ,
        KeyDelete  =>     KEY_DELETE ,
        KeyMacro  =>     KEY_MACRO ,
        KeyMute  =>     KEY_MUTE ,
        KeyVolumedown  =>     KEY_VOLUMEDOWN ,
        KeyVolumeup  =>     KEY_VOLUMEUP ,
        KeyPower  =>     KEY_POWER ,
        KeyKpequal  =>     KEY_KPEQUAL ,
        KeyKpplusminus  =>     KEY_KPPLUSMINUS ,
        KeyPause  =>     KEY_PAUSE ,
        KeyScale  =>     KEY_SCALE ,
        KeyKpcomma  =>     KEY_KPCOMMA ,
        KeyHangeul  =>     KEY_HANGEUL ,
        KeyHanja  =>     KEY_HANJA ,
        KeyYen  =>     KEY_YEN ,
        KeyLeftmeta  =>     KEY_LEFTMETA ,
        KeyRightmeta  =>     KEY_RIGHTMETA ,
        KeyCompose  =>     KEY_COMPOSE ,
        KeyStop  =>     KEY_STOP ,
        KeyAgain  =>     KEY_AGAIN ,
        KeyProps  =>     KEY_PROPS ,
        KeyUndo  =>     KEY_UNDO ,
        KeyFront  =>     KEY_FRONT ,
        KeyCopy  =>     KEY_COPY ,
        KeyOpen  =>     KEY_OPEN ,
        KeyPaste  =>     KEY_PASTE ,
        KeyFind  =>     KEY_FIND ,
        KeyCut  =>     KEY_CUT ,
        KeyHelp  =>     KEY_HELP ,
        KeyMenu  =>     KEY_MENU ,
        KeyCalc  =>     KEY_CALC ,
        KeySetup  =>     KEY_SETUP ,
        KeySleep  =>     KEY_SLEEP ,
        KeyWakeup  =>     KEY_WAKEUP ,
        KeyFile  =>     KEY_FILE ,
        KeySendfile  =>     KEY_SENDFILE ,
        KeyDeletefile  =>     KEY_DELETEFILE ,
        KeyXfer  =>     KEY_XFER ,
        KeyProg1  =>     KEY_PROG1 ,
        KeyProg2  =>     KEY_PROG2 ,
        KeyWww  =>     KEY_WWW ,
        KeyMsdos  =>     KEY_MSDOS ,
        KeyCoffee  =>     KEY_COFFEE ,
        KeyDirection  =>     KEY_DIRECTION ,
        KeyRotateDisplay  =>     KEY_ROTATE_DISPLAY ,
        KeyCyclewindows  =>     KEY_CYCLEWINDOWS ,
        KeyMail  =>     KEY_MAIL ,
        KeyBookmarks  =>     KEY_BOOKMARKS ,
        KeyComputer  =>     KEY_COMPUTER ,
        KeyBack  =>     KEY_BACK ,
        KeyForward  =>     KEY_FORWARD ,
        KeyClosecd  =>     KEY_CLOSECD ,
        KeyEjectcd  =>     KEY_EJECTCD ,
        KeyEjectclosecd  =>     KEY_EJECTCLOSECD ,
        KeyNextsong  =>     KEY_NEXTSONG ,
        KeyPlaypause  =>     KEY_PLAYPAUSE ,
        KeyPrevioussong  =>     KEY_PREVIOUSSONG ,
        KeyStopcd  =>     KEY_STOPCD ,
        KeyRecord  =>     KEY_RECORD ,
        KeyRewind  =>     KEY_REWIND ,
        KeyPhone  =>     KEY_PHONE ,
        KeyIso  =>     KEY_ISO ,
        KeyConfig  =>     KEY_CONFIG ,
        KeyHomepage  =>     KEY_HOMEPAGE ,
        KeyRefresh  =>     KEY_REFRESH ,
        KeyExit  =>     KEY_EXIT ,
        KeyMove  =>     KEY_MOVE ,
        KeyEdit  =>     KEY_EDIT ,
        KeyScrollup  =>     KEY_SCROLLUP ,
        KeyScrolldown  =>     KEY_SCROLLDOWN ,
        KeyKpleftparen  =>     KEY_KPLEFTPAREN ,
        KeyKprightparen  =>     KEY_KPRIGHTPAREN ,
        KeyNew  =>     KEY_NEW ,
        KeyRedo  =>     KEY_REDO ,
        KeyF13  =>     KEY_F13 ,
        KeyF14  =>     KEY_F14 ,
        KeyF15  =>     KEY_F15 ,
        KeyF16  =>     KEY_F16 ,
        KeyF17  =>     KEY_F17 ,
        KeyF18  =>     KEY_F18 ,
        KeyF19  =>     KEY_F19 ,
        KeyF20  =>     KEY_F20 ,
        KeyF21  =>     KEY_F21 ,
        KeyF22  =>     KEY_F22 ,
        KeyF23  =>     KEY_F23 ,
        KeyF24  =>     KEY_F24 ,
        KeyPlaycd  =>     KEY_PLAYCD ,
        KeyPausecd  =>     KEY_PAUSECD ,
        KeyProg3  =>     KEY_PROG3 ,
        KeyProg4  =>     KEY_PROG4 ,
        KeyDashboard  =>     KEY_DASHBOARD ,
        KeySuspend  =>     KEY_SUSPEND ,
        KeyClose  =>     KEY_CLOSE ,
        KeyPlay  =>     KEY_PLAY ,
        KeyFastforward  =>     KEY_FASTFORWARD ,
        KeyBassboost  =>     KEY_BASSBOOST ,
        KeyPrint  =>     KEY_PRINT ,
        KeyHp  =>     KEY_HP ,
        KeyCamera  =>     KEY_CAMERA ,
        KeySound  =>     KEY_SOUND ,
        KeyQuestion  =>     KEY_QUESTION ,
        KeyEmail  =>     KEY_EMAIL ,
        KeyChat  =>     KEY_CHAT ,
        KeySearch  =>     KEY_SEARCH ,
        KeyConnect  =>     KEY_CONNECT ,
        KeyFinance  =>     KEY_FINANCE ,
        KeySport  =>     KEY_SPORT ,
        KeyShop  =>     KEY_SHOP ,
        KeyAlterase  =>     KEY_ALTERASE ,
        KeyCancel  =>     KEY_CANCEL ,
        KeyBrightnessdown  =>     KEY_BRIGHTNESSDOWN ,
        KeyBrightnessup  =>     KEY_BRIGHTNESSUP ,
        KeyMedia  =>     KEY_MEDIA ,
        KeySwitchvideomode  =>     KEY_SWITCHVIDEOMODE ,
        KeyKbdillumtoggle  =>     KEY_KBDILLUMTOGGLE ,
        KeyKbdillumdown  =>     KEY_KBDILLUMDOWN ,
        KeyKbdillumup  =>     KEY_KBDILLUMUP ,
        KeySend  =>     KEY_SEND ,
        KeyReply  =>     KEY_REPLY ,
        KeyForwardmail  =>     KEY_FORWARDMAIL ,
        KeySave  =>     KEY_SAVE ,
        KeyDocuments  =>     KEY_DOCUMENTS ,
        KeyBattery  =>     KEY_BATTERY ,
        KeyBluetooth  =>     KEY_BLUETOOTH ,
        KeyWlan  =>     KEY_WLAN ,
        KeyUwb  =>     KEY_UWB ,
        KeyUnknown  =>     KEY_UNKNOWN ,
        KeyVideoNext  =>     KEY_VIDEO_NEXT ,
        KeyVideoPrev  =>     KEY_VIDEO_PREV ,
        KeyBrightnessCycle  =>     KEY_BRIGHTNESS_CYCLE ,
        KeyBrightnessAuto  =>     KEY_BRIGHTNESS_AUTO ,
        KeyDisplayOff  =>     KEY_DISPLAY_OFF ,
        KeyWwan  =>     KEY_WWAN ,
        KeyRfkill  =>     KEY_RFKILL ,
        KeyMicmute  =>     KEY_MICMUTE ,
        KeyOk  =>     KEY_OK ,
        KeySelect  =>     KEY_SELECT ,
        KeyGoto  =>     KEY_GOTO ,
        KeyClear  =>     KEY_CLEAR ,
        KeyPower2  =>     KEY_POWER2 ,
        KeyOption  =>     KEY_OPTION ,
        KeyInfo  =>     KEY_INFO ,
        KeyTime  =>     KEY_TIME ,
        KeyVendor  =>     KEY_VENDOR ,
        KeyArchive  =>     KEY_ARCHIVE ,
        KeyProgram  =>     KEY_PROGRAM ,
        KeyChannel  =>     KEY_CHANNEL ,
        KeyFavorites  =>     KEY_FAVORITES ,
        KeyEpg  =>     KEY_EPG ,
        KeyPvr  =>     KEY_PVR ,
        KeyMhp  =>     KEY_MHP ,
        KeyLanguage  =>     KEY_LANGUAGE ,
        KeyTitle  =>     KEY_TITLE ,
        KeySubtitle  =>     KEY_SUBTITLE ,
        KeyAngle  =>     KEY_ANGLE ,
        KeyZoom  =>     KEY_ZOOM ,
        KeyFullScreen  =>     KEY_FULL_SCREEN ,
        KeyMode  =>     KEY_MODE ,
        KeyKeyboard  =>     KEY_KEYBOARD ,
        KeyScreen  =>     KEY_SCREEN ,
        KeyPc  =>     KEY_PC ,
        KeyTv  =>     KEY_TV ,
        KeyTv2  =>     KEY_TV2 ,
        KeyVcr  =>     KEY_VCR ,
        KeyVcr2  =>     KEY_VCR2 ,
        KeySat  =>     KEY_SAT ,
        KeySat2  =>     KEY_SAT2 ,
        KeyCd  =>     KEY_CD ,
        KeyTape  =>     KEY_TAPE ,
        KeyRadio  =>     KEY_RADIO ,
        KeyTuner  =>     KEY_TUNER ,
        KeyPlayer  =>     KEY_PLAYER ,
        KeyText  =>     KEY_TEXT ,
        KeyDvd  =>     KEY_DVD ,
        KeyAux  =>     KEY_AUX ,
        KeyMp3  =>     KEY_MP3 ,
        KeyAudio  =>     KEY_AUDIO ,
        KeyVideo  =>     KEY_VIDEO ,
        KeyDirectory  =>     KEY_DIRECTORY ,
        KeyList  =>     KEY_LIST ,
        KeyMemo  =>     KEY_MEMO ,
        KeyCalendar  =>     KEY_CALENDAR ,
        KeyRed  =>     KEY_RED ,
        KeyGreen  =>     KEY_GREEN ,
        KeyYellow  =>     KEY_YELLOW ,
        KeyBlue  =>     KEY_BLUE ,
        KeyChannelup  =>     KEY_CHANNELUP ,
        KeyChanneldown  =>     KEY_CHANNELDOWN ,
        KeyFirst  =>     KEY_FIRST ,
        KeyLast  =>     KEY_LAST ,
        KeyAb  =>     KEY_AB ,
        KeyNext  =>     KEY_NEXT ,
        KeyRestart  =>     KEY_RESTART ,
        KeySlow  =>     KEY_SLOW ,
        KeyShuffle  =>     KEY_SHUFFLE ,
        KeyBreak  =>     KEY_BREAK ,
        KeyPrevious  =>     KEY_PREVIOUS ,
        KeyDigits  =>     KEY_DIGITS ,
        KeyTeen  =>     KEY_TEEN ,
        KeyTwen  =>     KEY_TWEN ,
        KeyVideophone  =>     KEY_VIDEOPHONE ,
        KeyGames  =>     KEY_GAMES ,
        KeyZoomin  =>     KEY_ZOOMIN ,
        KeyZoomout  =>     KEY_ZOOMOUT ,
        KeyZoomreset  =>     KEY_ZOOMRESET ,
        KeyWordprocessor  =>     KEY_WORDPROCESSOR ,
        KeyEditor  =>     KEY_EDITOR ,
        KeySpreadsheet  =>     KEY_SPREADSHEET ,
        KeyGraphicseditor  =>     KEY_GRAPHICSEDITOR ,
        KeyPresentation  =>     KEY_PRESENTATION ,
        KeyDatabase  =>     KEY_DATABASE ,
        KeyNews  =>     KEY_NEWS ,
        KeyVoicemail  =>     KEY_VOICEMAIL ,
        KeyAddressbook  =>     KEY_ADDRESSBOOK ,
        KeyMessenger  =>     KEY_MESSENGER ,
        KeyDisplaytoggle  =>     KEY_DISPLAYTOGGLE ,
        KeySpellcheck  =>     KEY_SPELLCHECK ,
        KeyLogoff  =>     KEY_LOGOFF ,
        KeyDollar  =>     KEY_DOLLAR ,
        KeyEuro  =>     KEY_EURO ,
        KeyFrameback  =>     KEY_FRAMEBACK ,
        KeyFrameforward  =>     KEY_FRAMEFORWARD ,
        KeyContextMenu  =>     KEY_CONTEXT_MENU ,
        KeyMediaRepeat  =>     KEY_MEDIA_REPEAT ,
        Key_10channelsup  =>     KEY_10CHANNELSUP ,
        Key_10channelsdown  =>     KEY_10CHANNELSDOWN ,
        KeyImages  =>     KEY_IMAGES ,
        KeyPickupPhone  =>     KEY_PICKUP_PHONE ,
        KeyHangupPhone  =>     KEY_HANGUP_PHONE ,
        KeyDelEol  =>     KEY_DEL_EOL ,
        KeyDelEos  =>     KEY_DEL_EOS ,
        KeyInsLine  =>     KEY_INS_LINE ,
        KeyDelLine  =>     KEY_DEL_LINE ,
        KeyFn  =>     KEY_FN ,
        KeyFnEsc  =>     KEY_FN_ESC ,
        KeyFnF1  =>     KEY_FN_F1 ,
        KeyFnF2  =>     KEY_FN_F2 ,
        KeyFnF3  =>     KEY_FN_F3 ,
        KeyFnF4  =>     KEY_FN_F4 ,
        KeyFnF5  =>     KEY_FN_F5 ,
        KeyFnF6  =>     KEY_FN_F6 ,
        KeyFnF7  =>     KEY_FN_F7 ,
        KeyFnF8  =>     KEY_FN_F8 ,
        KeyFnF9  =>     KEY_FN_F9 ,
        KeyFnF10  =>     KEY_FN_F10 ,
        KeyFnF11  =>     KEY_FN_F11 ,
        KeyFnF12  =>     KEY_FN_F12 ,
        KeyFn_1  =>     KEY_FN_1 ,
        KeyFn_2  =>     KEY_FN_2 ,
        KeyFnD  =>     KEY_FN_D ,
        KeyFnE  =>     KEY_FN_E ,
        KeyFnF  =>     KEY_FN_F ,
        KeyFnS  =>     KEY_FN_S ,
        KeyFnB  =>     KEY_FN_B ,
        KeyBrlDot1  =>     KEY_BRL_DOT1 ,
        KeyBrlDot2  =>     KEY_BRL_DOT2 ,
        KeyBrlDot3  =>     KEY_BRL_DOT3 ,
        KeyBrlDot4  =>     KEY_BRL_DOT4 ,
        KeyBrlDot5  =>     KEY_BRL_DOT5 ,
        KeyBrlDot6  =>     KEY_BRL_DOT6 ,
        KeyBrlDot7  =>     KEY_BRL_DOT7 ,
        KeyBrlDot8  =>     KEY_BRL_DOT8 ,
        KeyBrlDot9  =>     KEY_BRL_DOT9 ,
        KeyBrlDot10  =>     KEY_BRL_DOT10 ,
        KeyNumeric_0  =>     KEY_NUMERIC_0 ,
        KeyNumeric_1  =>     KEY_NUMERIC_1 ,
        KeyNumeric_2  =>     KEY_NUMERIC_2 ,
        KeyNumeric_3  =>     KEY_NUMERIC_3 ,
        KeyNumeric_4  =>     KEY_NUMERIC_4 ,
        KeyNumeric_5  =>     KEY_NUMERIC_5 ,
        KeyNumeric_6  =>     KEY_NUMERIC_6 ,
        KeyNumeric_7  =>     KEY_NUMERIC_7 ,
        KeyNumeric_8  =>     KEY_NUMERIC_8 ,
        KeyNumeric_9  =>     KEY_NUMERIC_9 ,
        KeyNumericStar  =>     KEY_NUMERIC_STAR ,
        KeyNumericPound  =>     KEY_NUMERIC_POUND ,
        KeyNumericA  =>     KEY_NUMERIC_A ,
        KeyNumericB  =>     KEY_NUMERIC_B ,
        KeyNumericC  =>     KEY_NUMERIC_C ,
        KeyNumericD  =>     KEY_NUMERIC_D ,
        KeyCameraFocus  =>     KEY_CAMERA_FOCUS ,
        KeyWpsButton  =>     KEY_WPS_BUTTON ,
        KeyTouchpadToggle  =>     KEY_TOUCHPAD_TOGGLE ,
        KeyTouchpadOn  =>     KEY_TOUCHPAD_ON ,
        KeyTouchpadOff  =>     KEY_TOUCHPAD_OFF ,
        KeyCameraZoomin  =>     KEY_CAMERA_ZOOMIN ,
        KeyCameraZoomout  =>     KEY_CAMERA_ZOOMOUT ,
        KeyCameraUp  =>     KEY_CAMERA_UP ,
        KeyCameraDown  =>     KEY_CAMERA_DOWN ,
        KeyCameraLeft  =>     KEY_CAMERA_LEFT ,
        KeyCameraRight  =>     KEY_CAMERA_RIGHT ,
        KeyAttendantOn  =>     KEY_ATTENDANT_ON ,
        KeyAttendantOff  =>     KEY_ATTENDANT_OFF ,
        KeyAttendantToggle  =>     KEY_ATTENDANT_TOGGLE ,
        KeyLightsToggle  =>     KEY_LIGHTS_TOGGLE ,
        KeyAlsToggle  =>     KEY_ALS_TOGGLE ,
        KeyButtonconfig  =>     KEY_BUTTONCONFIG ,
        KeyTaskmanager  =>     KEY_TASKMANAGER ,
        KeyJournal  =>     KEY_JOURNAL ,
        KeyControlpanel  =>     KEY_CONTROLPANEL ,
        KeyAppselect  =>     KEY_APPSELECT ,
        KeyScreensaver  =>     KEY_SCREENSAVER ,
        KeyVoicecommand  =>     KEY_VOICECOMMAND ,
        KeyAssistant  =>     KEY_ASSISTANT ,
        KeyKbdLayoutNext  =>     KEY_KBD_LAYOUT_NEXT ,
        KeyBrightnessMin  =>     KEY_BRIGHTNESS_MIN ,
        KeyBrightnessMax  =>     KEY_BRIGHTNESS_MAX ,
        KeyKbdinputassistPrev  =>     KEY_KBDINPUTASSIST_PREV ,
        KeyKbdinputassistNext  =>     KEY_KBDINPUTASSIST_NEXT ,
        KeyKbdinputassistPrevgroup  =>     KEY_KBDINPUTASSIST_PREVGROUP ,
        KeyKbdinputassistNextgroup  =>     KEY_KBDINPUTASSIST_NEXTGROUP ,
        KeyKbdinputassistAccept  =>     KEY_KBDINPUTASSIST_ACCEPT ,
        KeyKbdinputassistCancel  =>     KEY_KBDINPUTASSIST_CANCEL ,
        KeyRightUp  =>     KEY_RIGHT_UP ,
        KeyRightDown  =>     KEY_RIGHT_DOWN ,
        KeyLeftUp  =>     KEY_LEFT_UP ,
        KeyLeftDown  =>     KEY_LEFT_DOWN ,
        KeyRootMenu  =>     KEY_ROOT_MENU ,
        KeyMediaTopMenu  =>     KEY_MEDIA_TOP_MENU ,
        KeyNumeric_11  =>     KEY_NUMERIC_11 ,
        KeyNumeric_12  =>     KEY_NUMERIC_12 ,
        KeyAudioDesc  =>     KEY_AUDIO_DESC ,
        Key_3dMode  =>     KEY_3D_MODE ,
        KeyNextFavorite  =>     KEY_NEXT_FAVORITE ,
        KeyStopRecord  =>     KEY_STOP_RECORD ,
        KeyPauseRecord  =>     KEY_PAUSE_RECORD ,
        KeyVod  =>     KEY_VOD ,
        KeyUnmute  =>     KEY_UNMUTE ,
        KeyFastreverse  =>     KEY_FASTREVERSE ,
        KeySlowreverse  =>     KEY_SLOWREVERSE ,
        KeyData  =>     KEY_DATA ,
        KeyOnscreenKeyboard  =>     KEY_ONSCREEN_KEYBOARD ,
        KeyPrivacyScreenToggle  =>     KEY_PRIVACY_SCREEN_TOGGLE ,
        KeySelectiveScreenshot  =>     KEY_SELECTIVE_SCREENSHOT ,
    }
    midi {
        PitchWheel,
        Note,
        ControlChange,
        ProgramChange,
        Aftertouch,
        PolyAftertouch
    }
}

#[cfg(feature = "midi")]
impl From<MidiMessageType> for MappedCtlsMidi {
    fn from(value: MidiMessageType) -> Self {
        match value {
            MidiMessageType::PitchWheel => MappedCtlsMidi::PitchWheel,
            MidiMessageType::ControlChange => MappedCtlsMidi::ControlChange,
            MidiMessageType::NoteOn | MidiMessageType::NoteOff => MappedCtlsMidi::Note,
            MidiMessageType::Aftertouch => MappedCtlsMidi::Aftertouch,
            MidiMessageType::PolyAftertouch => MappedCtlsMidi::PolyAftertouch,
            MidiMessageType::ProgramChange => MappedCtlsMidi::ProgramChange,
        }
    }
}

#[cfg(feature = "midi")]
impl From<MappedCtlsMidi> for MappedCtls {
    fn from(value: MappedCtlsMidi) -> Self {
        match value {
            MappedCtlsMidi::PitchWheel => MappedCtls::PitchWheel,
            MappedCtlsMidi::Note => MappedCtls::Note,
            MappedCtlsMidi::ControlChange => MappedCtls::ControlChange,
            MappedCtlsMidi::ProgramChange => MappedCtls::ProgramChange,
            MappedCtlsMidi::Aftertouch => MappedCtls::Aftertouch,
            MappedCtlsMidi::PolyAftertouch => MappedCtls::PolyAftertouch,
            MappedCtlsMidi::Unhandled => MappedCtls::Unhandled,
        }
    }
}

#[cfg(feature = "midi")]
impl TryFrom<MappedCtls> for MappedCtlsMidi {
    type Error = String;

    fn try_from(value: MappedCtls) -> std::result::Result<Self, Self::Error> {
        match value {
            MappedCtls::PitchWheel => Ok(MappedCtlsMidi::PitchWheel),
            MappedCtls::Note => Ok(MappedCtlsMidi::Note),
            MappedCtls::ControlChange => Ok(MappedCtlsMidi::ControlChange),
            MappedCtls::ProgramChange => Ok(MappedCtlsMidi::ProgramChange),
            MappedCtls::Aftertouch => Ok(MappedCtlsMidi::Aftertouch),
            MappedCtls::PolyAftertouch => Ok(MappedCtlsMidi::PolyAftertouch),
            MappedCtls::Unhandled => Ok(MappedCtlsMidi::Unhandled),
            _ => Err(format!("Control type {value} is not a MIDI control")),
        }
    }
}

impl MappedCtls {
    pub(crate) fn get_predefined_control_cfg(&self) -> ControlMatchers {
        #[cfg(feature = "midi")]
        if self.is_a_midi_control() {
            use crate::midi::MIDIv1_CONTROL_INTERVAL;

            let mut mc = MidiControlMatcherCfg::default();
            mc.midi_message.r#type = (*self).try_into().expect("Only MIDI control types are expected here.");
            mc.from_predefined = "_ Generated _ ".to_string();
            mc.range = MIDIv1_CONTROL_INTERVAL;
            match mc.midi_message.r#type {
                MappedCtlsMidi::PitchWheel => {
                    mc.range = MIDIv1_PITCH_WHEEL_INTERVAL;
                }
                MappedCtlsMidi::Note => mc.midi_message.number = MidiNumberCfg::Single(0),
                MappedCtlsMidi::ControlChange => mc.midi_message.number = MidiNumberCfg::Single(1),
                MappedCtlsMidi::ProgramChange => mc.midi_message.number = MidiNumberCfg::Single(1),
                MappedCtlsMidi::Aftertouch => mc.midi_message.number = MidiNumberCfg::Single(0),
                MappedCtlsMidi::PolyAftertouch => mc.midi_message.number = MidiNumberCfg::Single(0),
                MappedCtlsMidi::Unhandled => unreachable!(),
            }
            return ControlMatchers::Midi(mc);
        }

        ControlMatchers::Hid({
            let mut hc = HidControlMatcherCfg::default();
            hc.from_predefined = "_ Generated _ ".to_string();
            if self.is_absolute() || self.is_relative() {
                hc.initial_value = 0.0;
                hc.range = {
                    match self {
                        Self::AbsX | Self::AbsY | Self::AbsZ => HID_AXIS_MAX_INTERVAL,
                        _ => NumInterval::new(0.0 as BaseNumT, 127.0),
                    }
                };
            } else if self.is_button() || self.is_key() {
                hc.range = NumInterval::new(0.0 as BaseNumT, 2.9);
            } else {
                hc.range = NumInterval::new(0.0 as BaseNumT, 127.0);
            }
            hc
        })
    }

    pub(crate) fn is_a_midi_control(&self) -> bool {
        matches!(
            self,
            MappedCtls::PitchWheel
                | MappedCtls::Note
                | MappedCtls::ControlChange
                | MappedCtls::ProgramChange
                | MappedCtls::Aftertouch
                | MappedCtls::PolyAftertouch
        )
    }

    pub(crate) fn is_a_mouse_control(&self) -> bool {
        self.is_relative()
            || matches!(
                self,
                MappedCtls::BtnLeft
                    | MappedCtls::BtnRight
                    | MappedCtls::BtnMiddle
                    | MappedCtls::BtnSide
                    | MappedCtls::BtnExtra
            )
    }

    pub(crate) fn is_a_keyboard_control(&self) -> bool {
        self.is_key()
    }

    pub(crate) fn is_a_joystick_control(&self) -> bool {
        self.is_absolute()
            || self.is_special_force_feedback_x()
            || self.is_special_force_feedback_y()
            || matches!(self, MappedCtls::BtnJoystick | MappedCtls::BtnTrigger)
    }

    pub(crate) fn is_a_gamepad_control(&self) -> bool {
        self.is_button() && !self.is_a_mouse_control() && !self.is_a_joystick_control()
    }

    pub(crate) fn is_a_misc_control(&self) -> bool {
        !(self.is_a_gamepad_control()
            || self.is_a_joystick_control()
            || self.is_a_keyboard_control()
            || self.is_a_mouse_control())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_absolute_iterator() {
        let abs_controls: Vec<MappedCtls> = MappedCtls::iter_absolute().collect();

        // Check we got all absolute controls
        assert!(!abs_controls.is_empty());

        // Verify all are absolute
        for control in &abs_controls {
            assert!(control.is_absolute(), "{:?} should be absolute", control);
            assert!(!control.is_relative(), "{:?} should not be relative", control);
            assert!(!control.is_button(), "{:?} should not be button", control);
        }

        // Check specific controls are present
        assert!(abs_controls.contains(&MappedCtls::AbsX));
        assert!(abs_controls.contains(&MappedCtls::AbsY));
        assert!(abs_controls.contains(&MappedCtls::AbsZ));
        assert!(abs_controls.contains(&MappedCtls::AbsBrake));
        assert!(abs_controls.contains(&MappedCtls::AbsGas));
    }

    #[test]
    fn test_relative_iterator() {
        let rel_controls: Vec<MappedCtls> = MappedCtls::iter_relative().collect();

        // Check we got all relative controls
        assert!(!rel_controls.is_empty());

        // Verify all are relative
        for control in &rel_controls {
            assert!(control.is_relative(), "{:?} should be relative", control);
            assert!(!control.is_absolute(), "{:?} should not be absolute", control);
            assert!(!control.is_button(), "{:?} should not be button", control);
        }

        // Check specific controls are present
        assert!(rel_controls.contains(&MappedCtls::RelX));
        assert!(rel_controls.contains(&MappedCtls::RelY));
        assert!(rel_controls.contains(&MappedCtls::RelWheel));
        assert!(rel_controls.contains(&MappedCtls::RelHwheel));
    }

    #[test]
    fn test_button_iterator() {
        let btn_controls: Vec<MappedCtls> = MappedCtls::iter_button().collect();

        // Check we got all button controls
        assert!(!btn_controls.is_empty());

        // Verify all are buttons
        for control in &btn_controls {
            assert!(control.is_button(), "{:?} should be button", control);
            assert!(!control.is_relative(), "{:?} should not be relative", control);
        }

        // Check specific controls are present
        assert!(btn_controls.contains(&MappedCtls::BtnSouth));
        assert!(btn_controls.contains(&MappedCtls::BtnEast));
        assert!(btn_controls.contains(&MappedCtls::BtnWest));
        assert!(btn_controls.contains(&MappedCtls::BtnNorth));
        assert!(btn_controls.contains(&MappedCtls::BtnLeft));
        assert!(btn_controls.contains(&MappedCtls::BtnRight));
    }

    #[test]
    fn test_iterator_exact_size() {
        let abs_iter = MappedCtls::iter_absolute();
        let abs_count = abs_iter.len();
        assert_eq!(abs_count, abs_iter.count());

        let rel_iter = MappedCtls::iter_relative();
        let rel_count = rel_iter.len();
        assert_eq!(rel_count, rel_iter.count());

        let btn_iter = MappedCtls::iter_button();
        let btn_count = btn_iter.len();
        assert_eq!(btn_count, btn_iter.count());
    }

    #[test]
    fn test_iterator_next_behavior() {
        let mut iter = MappedCtls::iter_button();
        let iter_size_lower_bound = iter.size_hint().0;

        // Get first item
        let first = iter.next();
        assert!(first.is_some());

        // Get second item
        let second = iter.next();
        assert!(second.is_some());

        // First and second should be different
        assert_ne!(first, second);

        // Should eventually return None
        let mut count = 2;
        while iter.next().is_some() {
            count += 1;
            if count > iter_size_lower_bound {
                panic!("Iterator didn't terminate");
            }
        }

        // After None, should keep returning None
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_from_evdev_absolute() {
        use evdev::{AbsoluteAxisCode, EventType, InputEvent};

        let event = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 100);
        let control: MappedCtls = event.into();
        assert_eq!(control, MappedCtls::AbsX);

        let event = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 200);
        let control: MappedCtls = event.into();
        assert_eq!(control, MappedCtls::AbsY);

        let event = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_BRAKE.0, 50);
        let control: MappedCtls = event.into();
        assert_eq!(control, MappedCtls::AbsBrake);
    }

    #[test]
    fn test_from_evdev_relative() {
        use evdev::{EventType, InputEvent, RelativeAxisCode};

        let event = InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, 10);
        let control: MappedCtls = event.into();
        assert_eq!(control, MappedCtls::RelX);

        let event = InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_WHEEL.0, -1);
        let control: MappedCtls = event.into();
        assert_eq!(control, MappedCtls::RelWheel);
    }

    #[test]
    fn test_from_evdev_button() {
        use evdev::{EventType, InputEvent, KeyCode};

        let event = InputEvent::new(EventType::KEY.0, KeyCode::BTN_SOUTH.0, 1);
        let control: MappedCtls = event.into();
        assert!(control == MappedCtls::BtnSouth || control == MappedCtls::BtnGamepad);

        let event = InputEvent::new(EventType::KEY.0, KeyCode::BTN_LEFT.0, 1);
        let control: MappedCtls = event.into();
        assert_eq!(control, MappedCtls::BtnLeft);
    }

    #[test]
    fn test_to_u16_absolute() {
        use evdev::AbsoluteAxisCode;

        let code: u16 = MappedCtls::AbsX.into();
        assert_eq!(code, AbsoluteAxisCode::ABS_X.0);

        let code: u16 = MappedCtls::AbsY.into();
        assert_eq!(code, AbsoluteAxisCode::ABS_Y.0);

        let code: u16 = MappedCtls::AbsGas.into();
        assert_eq!(code, AbsoluteAxisCode::ABS_GAS.0);
    }

    #[test]
    fn test_to_u16_relative() {
        use evdev::RelativeAxisCode;

        let code: u16 = MappedCtls::RelX.into();
        assert_eq!(code, RelativeAxisCode::REL_X.0);

        let code: u16 = MappedCtls::RelWheel.into();
        assert_eq!(code, RelativeAxisCode::REL_WHEEL.0);
    }

    #[test]
    fn test_to_u16_button() {
        use evdev::KeyCode;

        let code: u16 = MappedCtls::BtnSouth.into();
        assert_eq!(code, KeyCode::BTN_SOUTH.0);

        let code: u16 = MappedCtls::BtnLeft.into();
        assert_eq!(code, KeyCode::BTN_LEFT.0);
    }

    #[test]
    fn test_roundtrip_conversion() {
        use evdev::{EventType, InputEvent};

        // Test absolute
        let original = MappedCtls::AbsX;
        let code: u16 = original.into();
        let event = InputEvent::new(EventType::ABSOLUTE.0, code, 0);
        let converted: MappedCtls = event.into();
        assert_eq!(original, converted);

        // Test relative
        let original = MappedCtls::RelWheel;
        let code: u16 = original.into();
        let event = InputEvent::new(EventType::RELATIVE.0, code, 0);
        let converted: MappedCtls = event.into();
        assert_eq!(original, converted);

        // Test button
        let original = MappedCtls::BtnSouth;
        let code: u16 = original.into();
        let event = InputEvent::new(EventType::KEY.0, code, 0);
        let converted: MappedCtls = event.into();
        assert!(original == converted || converted == MappedCtls::BtnGamepad);
    }

    #[test]
    fn test_helper_methods() {
        // Test absolute
        assert!(MappedCtls::AbsX.is_absolute());
        assert!(!MappedCtls::AbsX.is_relative());
        assert!(!MappedCtls::AbsX.is_button());

        // Test relative
        assert!(!MappedCtls::RelX.is_absolute());
        assert!(MappedCtls::RelX.is_relative());
        assert!(!MappedCtls::RelX.is_button());

        // Test button
        assert!(!MappedCtls::BtnSouth.is_absolute());
        assert!(!MappedCtls::BtnSouth.is_relative());
        assert!(MappedCtls::BtnSouth.is_button());

        // Test unhandled
        assert!(!MappedCtls::Unhandled.is_absolute());
        assert!(!MappedCtls::Unhandled.is_relative());
        assert!(!MappedCtls::Unhandled.is_button());
        assert!(MappedCtls::Unhandled.is_unhandled());
    }

    #[test]
    fn test_string_parsing() {
        use std::str::FromStr;

        // Test parsing absolute
        assert_eq!(MappedCtls::from_str("ABS_X").unwrap(), MappedCtls::AbsX);
        assert_eq!(MappedCtls::from_str("ABS_BRAKE").unwrap(), MappedCtls::AbsBrake);

        // Test parsing relative
        assert_eq!(MappedCtls::from_str("REL_X").unwrap(), MappedCtls::RelX);
        assert_eq!(MappedCtls::from_str("REL_WHEEL").unwrap(), MappedCtls::RelWheel);

        // Test parsing button
        assert_eq!(MappedCtls::from_str("BTN_SOUTH").unwrap(), MappedCtls::BtnSouth);
        assert_eq!(MappedCtls::from_str("BTN_LEFT").unwrap(), MappedCtls::BtnLeft);

        // Test invalid string
        assert!(MappedCtls::from_str("INVALID").is_err());
    }

    #[test]
    fn test_to_string() {
        assert_eq!(MappedCtls::AbsX.to_string(), "ABS_X");
        assert_eq!(MappedCtls::RelWheel.to_string(), "REL_WHEEL");
        assert_eq!(MappedCtls::BtnSouth.to_string(), "BTN_SOUTH");
        assert_eq!(MappedCtls::Unhandled.to_string(), "UNHANDLED");
    }

    #[test]
    fn test_serialization() {
        let control = MappedCtls::AbsX;
        let serialized = serde_saphyr::to_string(&control).unwrap();
        assert_eq!(serialized, "ABS_X\n");

        let control = MappedCtls::BtnSouth;
        let serialized = serde_saphyr::to_string(&control).unwrap();
        assert_eq!(serialized, "BTN_SOUTH\n");
    }

    #[test]
    fn test_deserialization() {
        let yaml = "ABS_X\n";
        let control: MappedCtls = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(control, MappedCtls::AbsX);

        let yaml = "BTN_SOUTH\n";
        let control: MappedCtls = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(control, MappedCtls::BtnSouth);

        let yaml = "INVALID\n";
        let result: Result<MappedCtls, _> = serde_saphyr::from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_clone_and_copy() {
        let original = MappedCtls::AbsX;
        #[allow(clippy::clone_on_copy)]
        let cloned = original.clone();
        let copied = original;

        assert_eq!(original, cloned);
        assert_eq!(original, copied);
    }

    #[test]
    fn test_default() {
        let default = MappedCtls::default();
        assert_eq!(default, MappedCtls::Unhandled);
    }

    #[test]
    fn test_multitouch_controls() {
        // Test that multitouch controls are properly included
        let abs_controls: Vec<MappedCtls> = MappedCtls::iter_absolute().collect();

        assert!(abs_controls.contains(&MappedCtls::AbsMtSlot));
        assert!(abs_controls.contains(&MappedCtls::AbsMtTouchMajor));
        assert!(abs_controls.contains(&MappedCtls::AbsMtPositionX));
        assert!(abs_controls.contains(&MappedCtls::AbsMtPositionY));
    }

    #[test]
    fn test_iterator_clone() {
        // for b in MappedCtls::iter_button() {
        //     println!("{b:?} {b}");
        // }

        // for a in MappedCtls::iter_absolute() {
        //     println!("{a:?} {a}");
        // }

        let iter1 = MappedCtls::iter_button();
        let mut iter2 = iter1.clone();

        // Advance iter2
        iter2.next();
        iter2.next();

        // iter1 should still be at the beginning
        let count1 = iter1.count();
        let count2 = iter2.count();

        assert_eq!(count1, count2 + 2);
    }

    #[test]
    fn test_no_duplicates_in_iterators() {
        use std::collections::HashSet;

        // Check absolute controls
        let abs_controls: Vec<MappedCtls> = MappedCtls::iter_absolute().collect();
        let abs_set: HashSet<_> = abs_controls.iter().collect();
        assert_eq!(abs_controls.len(), abs_set.len(), "Duplicate in absolute iterator");

        // Check relative controls
        let rel_controls: Vec<MappedCtls> = MappedCtls::iter_relative().collect();
        let rel_set: HashSet<_> = rel_controls.iter().collect();
        assert_eq!(rel_controls.len(), rel_set.len(), "Duplicate in relative iterator");

        // Check button controls
        let btn_controls: Vec<MappedCtls> = MappedCtls::iter_button().collect();
        let btn_set: HashSet<_> = btn_controls.iter().collect();
        assert_eq!(btn_controls.len(), btn_set.len(), "Duplicate in button iterator");
    }

    #[test]
    fn test_unhandled_conversion() {
        let code: u16 = MappedCtls::Unhandled.into();
        assert_eq!(code, 0);

        // MIDI controls should also convert to 0 (unimplemented)
        let code: u16 = MappedCtls::PitchWheel.into();
        assert_eq!(code, 0);
    }
}
