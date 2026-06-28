#[derive(Default, PartialEq, Eq)]
pub struct KeyboardModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub caps_lock: bool,
    pub logo: bool,
    pub num_lock: bool,
}

impl From<&smithay::input::keyboard::ModifiersState> for KeyboardModifiers {
    fn from(value: &smithay::input::keyboard::ModifiersState) -> Self {
        Self {
            ctrl: value.ctrl,
            alt: value.alt,
            shift: value.shift,
            caps_lock: value.caps_lock,
            logo: value.logo,
            num_lock: value.num_lock,
        }
    }
}

pub enum ShortcutAction {
    Command(String),
    SystemAction(()),
    Nothing,
}

pub struct RegisteredShortcut {
    pub modifiers: KeyboardModifiers,
    pub keysyms: Vec<u32>,
    pub action: ShortcutAction,
}

impl RegisteredShortcut {
    pub fn execute(&self) -> anyhow::Result<()> {
        match self.action {
            ShortcutAction::Command(ref command) => {
                std::process::Command::new(command).spawn()?;
            }
            _ => todo!(),
        }
        Ok(())
    }
}

pub struct KeystrokeEvent {
    pub modifiers: KeyboardModifiers,
    pub modified_keysyms: Vec<u32>,
}

impl KeystrokeEvent {
    pub fn valid_for_shortcut(&self, shortcut: &RegisteredShortcut) -> bool {
        self.modifiers == shortcut.modifiers && self.modified_keysyms == shortcut.keysyms
    }
}

#[derive(Default)]
pub struct ShortcutsComponent {
    registry: Vec<RegisteredShortcut>,
}

impl ShortcutsComponent {
    pub fn register(&mut self, shortcut: RegisteredShortcut) {
        self.registry.push(shortcut);
    }

    pub fn find_shortcut(
        &self,
        modifiers: KeyboardModifiers,
        keysyms_delta: Vec<xkeysym::Keysym>,
    ) -> Option<&RegisteredShortcut> {
        let modified_keysyms = keysyms_delta
            .iter()
            .map(|keysym| keysym.raw())
            .collect::<Vec<_>>();
        let event = KeystrokeEvent {
            modifiers,
            modified_keysyms,
        };
        self.registry.iter().find(|s| event.valid_for_shortcut(s))
    }
}
